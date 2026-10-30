//! Windows process containment for a caller-authorized analyzer executable.
//! Setup API calls are synchronous; the deadline bounds the running job loop and cleanup waits.
use std::{
    ffi::OsStr,
    io::{self, Read},
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{self, SyncSender, TrySendError},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_FAILED, WAIT_OBJECT_0,
    },
    Security::SECURITY_ATTRIBUTES,
    System::{
        JobObjects::{
            CreateJobObjectW, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Pipes::CreatePipe,
        Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, PROC_THREAD_ATTRIBUTE_JOB_LIST,
            InitializeProcThreadAttributeList, ResumeThread, TerminateProcess,
            UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
            EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            STARTF_USESTDHANDLES, STARTUPINFOEXW,
        },
    },
};
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Timeout {
        cleanup_confirmed: bool,
        total_processes: u32,
    },
    System {
        message: String,
        cleanup_confirmed: Option<bool>,
    },
    Panic {
        cleanup_confirmed: bool,
    },
    Backpressure {
        cleanup_confirmed: bool,
    },
    Cancelled {
        cleanup_confirmed: bool,
    },
    Pipe {
        message: String,
        cleanup_confirmed: bool,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub struct Event {
    pub stderr: bool,
    pub bytes: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub code: u32,
    pub active: u32,
}
fn ioerr() -> Failure {
    Failure::System {
        message: io::Error::last_os_error().to_string(),
        cleanup_confirmed: None,
    }
}
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
fn quoted(s: &OsStr) -> Vec<u16> {
    let mut out = vec![34];
    let mut slash = 0;
    for c in s.encode_wide() {
        if c == 92 {
            slash += 1;
            continue;
        }
        if c == 34 {
            out.extend(std::iter::repeat(92).take(slash * 2 + 1));
            out.push(c)
        } else {
            out.extend(std::iter::repeat(92).take(slash));
            out.push(c)
        }
        slash = 0
    }
    out.extend(std::iter::repeat(92).take(slash * 2));
    out.push(34);
    out
}
unsafe fn owned(h: HANDLE) -> Result<OwnedHandle, Failure> {
    // SAFETY: The caller supplies null or a uniquely owned valid handle; null is rejected.
    unsafe {
        if h.is_null() {
            Err(ioerr())
        } else {
            Ok(OwnedHandle::from_raw_handle(h))
        }
    }
}
fn h(x: &OwnedHandle) -> HANDLE {
    x.as_raw_handle() as HANDLE
}
// Owns both raw CreateProcess handles immediately, including before job assignment.
struct SuspendedChild {
    process: HANDLE,
    thread: HANDLE,
    assigned: bool,
}
impl SuspendedChild {
    unsafe fn terminate_unassigned(&mut self) -> bool {
        // SAFETY: The guard owns a valid process handle until it is closed.
        unsafe {
            if self.process.is_null() {
                return false;
            }
            let _ = TerminateProcess(self.process, 1);
            WaitForSingleObject(self.process, 5000) == WAIT_OBJECT_0
        }
    }
}
impl Drop for SuspendedChild {
    fn drop(&mut self) {
        // SAFETY: this guard owns both process handles and closes them only once.
        unsafe {
            if !self.assigned && !self.process.is_null() {
                let _ = self.terminate_unassigned();
            }
            if !self.thread.is_null() {
                CloseHandle(self.thread);
            }
            if !self.process.is_null() {
                CloseHandle(self.process);
            }
        }
    }
}
unsafe fn pipe() -> Result<(OwnedHandle, OwnedHandle), Failure> {
    // SAFETY: Windows initializes both handles on success; OwnedHandle owns each exactly once.
    unsafe {
        let (mut r, mut w) = (null_mut(), null_mut());
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        if CreatePipe(&mut r, &mut w, &sa, 0) == 0 {
            return Err(ioerr());
        }
        let r = owned(r)?;
        let w = owned(w)?;
        if SetHandleInformation(h(&r), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(ioerr());
        }
        Ok((r, w))
    }
}
struct Attributes {
    buf: Vec<usize>,
}
impl Attributes {
    unsafe fn new() -> Result<Self, Failure> {
        // SAFETY: The attribute buffer is aligned and remains alive until DeleteProcThreadAttributeList.
        unsafe {
            let mut size = 0;
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size);
            if size == 0 {
                return Err(ioerr());
            }
            let mut buf = vec![0usize; (size + size_of::<usize>() - 1) / size_of::<usize>()];
            if InitializeProcThreadAttributeList(buf.as_mut_ptr() as _, 2, 0, &mut size) == 0 {
                return Err(ioerr());
            }
            Ok(Self { buf })
        }
    }
    fn ptr(&mut self) -> *mut core::ffi::c_void {
        self.buf.as_mut_ptr() as _
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: this list was initialized successfully and remains backed by self.buf.
        unsafe { DeleteProcThreadAttributeList(self.ptr()) }
    }
}
unsafe fn accounting(job: HANDLE) -> Result<(u32, u32), Failure> {
    // SAFETY: The caller holds a live job handle and the output struct is correctly sized.
    unsafe {
        let mut v: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
        if QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            &mut v as *mut _ as _,
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        ) == 0
        {
            return Err(ioerr());
        }
        Ok((v.ActiveProcesses, v.TotalProcesses))
    }
}
unsafe fn terminate(job: HANDLE) -> bool {
    // SAFETY: The caller holds a live job handle; only its owned job is terminated.
    unsafe {
        if matches!(accounting(job), Ok((0, _))) {
            return true;
        }
        if TerminateJobObject(job, 1) == 0 {
            return false;
        }
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match accounting(job) {
                Ok((0, _)) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
            if Instant::now() >= end {
                return false;
            }
            thread::sleep(Duration::from_millis(10))
        }
    }
}
enum Message {
    Data(Event),
    Closed,
    Error(String),
}
enum LoopExit {
    Success(u32),
    Timeout(u32),
    Backpressure,
    Cancelled,
    Pipe(String),
    System(String),
}
fn drain(
    handle: OwnedHandle,
    stderr: bool,
    tx: mpsc::SyncSender<Message>,
) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().spawn(move || {
        let mut file = std::fs::File::from(handle);
        let mut b = [0u8; 8192];
        loop {
            match file.read(&mut b) {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .send(Message::Data(Event {
                            stderr,
                            bytes: b[..n].to_vec(),
                        }))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(e.to_string()));
                    return;
                }
            }
        }
        let _ = tx.send(Message::Closed);
    })
}
fn join_bounded(threads: Vec<thread::JoinHandle<()>>) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while threads.iter().any(|t| !t.is_finished()) {
        if Instant::now() >= end {
            return false;
        }
        thread::sleep(Duration::from_millis(10))
    }
    threads.into_iter().all(|t| t.join().is_ok())
}
/// Trial: creates a suspended child with job-list assignment, then resumes.
/// Output delivery is nonblocking; a full/disconnected consumer terminates the owned job.
pub fn run(
    exe: &Path,
    args: &[&str],
    deadline: Duration,
    events: SyncSender<Event>,
    cancel: &AtomicBool,
) -> Result<Outcome, Failure> {
    run_inner(exe, args, deadline, events, cancel)
}
fn run_inner(
    exe: &Path,
    args: &[&str],
    deadline: Duration,
    events: SyncSender<Event>,
    cancel: &AtomicBool,
) -> Result<Outcome, Failure> {
    // SAFETY: Win32 receives live RAII-owned handles and correctly sized buffers;
    // the attribute list and allowlisted inheritable handles outlive CreateProcessW;
    // child, job and pipe handles remain owned through assignment, waits and cleanup.
    unsafe {
        let started = Instant::now();
        if cancel.load(Ordering::Acquire) {
            return Err(Failure::Cancelled {
                cleanup_confirmed: true,
            });
        }
        if exe.as_os_str().encode_wide().any(|c| c == 0) || args.iter().any(|a| a.contains('\0')) {
            return Err(Failure::System {
                message: "embedded NUL argument".into(),
                cleanup_confirmed: None,
            });
        }
        let job = owned(CreateJobObjectW(null(), null()))?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            h(&job),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(ioerr());
        }
        let (out_r, out_w) = pipe()?;
        let (err_r, err_w) = pipe()?;
        let (in_r, in_w) = pipe()?;
        if SetHandleInformation(h(&in_r), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
            return Err(ioerr());
        }
        if SetHandleInformation(h(&in_w), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(ioerr());
        }
        let handles = [h(&out_w), h(&err_w), h(&in_r)];
        let creation_jobs = [h(&job)];
        let mut attr = Attributes::new()?;
        if UpdateProcThreadAttribute(
            attr.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr() as _,
            size_of_val(&handles),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(ioerr());
        }
        // Both attribute-value arrays outlive the attribute list on all exit paths.
        if UpdateProcThreadAttribute(
            attr.ptr(), 0, PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            creation_jobs.as_ptr() as _, size_of_val(&creation_jobs), null_mut(), null(),
        ) == 0 { return Err(ioerr()); }
        let mut si: STARTUPINFOEXW = zeroed();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = h(&in_r);
        si.StartupInfo.hStdOutput = h(&out_w);
        si.StartupInfo.hStdError = h(&err_w);
        si.lpAttributeList = attr.ptr();
        let application = wide(exe.as_os_str());
        let mut cmd = quoted(exe.as_os_str());
        for arg in args {
            cmd.push(32);
            cmd.extend(quoted(OsStr::new(arg)))
        }
        cmd.push(0);
        let mut pi: PROCESS_INFORMATION = zeroed();
        if CreateProcessW(
            application.as_ptr(),
            cmd.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            null(),
            null(),
            &si.StartupInfo,
            &mut pi,
        ) == 0
        {
            return Err(ioerr());
        }
        let mut child = SuspendedChild {
            process: pi.hProcess,
            thread: pi.hThread,
            assigned: true,
        };
        if child.process.is_null() || child.thread.is_null() {
            let cleaned = terminate(h(&job));
            return Err(Failure::System {
                message: "CreateProcess returned null handle".into(),
                cleanup_confirmed: Some(cleaned),
            });
        }
        let launch =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), Failure> {
                if ResumeThread(child.thread) == u32::MAX {
                    let message = format!("resume: {:?}", io::Error::last_os_error());
                    let cleaned = terminate(h(&job));
                    return Err(Failure::System {
                        message,
                        cleanup_confirmed: Some(cleaned),
                    });
                }
                Ok(())
            }));
        match launch {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                let cleaned = if child.assigned {
                    terminate(h(&job))
                } else {
                    let ok = child.terminate_unassigned();
                    child.assigned = true;
                    ok
                };
                return Err(Failure::Panic {
                    cleanup_confirmed: cleaned,
                });
            }
        }
        drop(out_w);
        drop(err_w);
        drop(in_r);
        drop(in_w);
        drop(attr);
        let (tx, rx) = mpsc::sync_channel(16);
        let mut threads = Vec::new();
        let setup =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                let first = drain(out_r, false, tx.clone());
                threads.push(first.map_err(|e| format!("stdout reader: {e}"))?);

                let second = drain(err_r, true, tx.clone());
                threads.push(second.map_err(|e| format!("stderr reader: {e}"))?);
                Ok(())
            }));
        drop(tx);
        match setup {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                let killed = terminate(h(&job));
                drop(rx);
                let joined = join_bounded(threads);
                return Err(Failure::System {
                    message,
                    cleanup_confirmed: Some(killed && joined),
                });
            }
            Err(_) => {
                let killed = terminate(h(&job));
                drop(rx);
                let joined = join_bounded(threads);
                return Err(Failure::Panic {
                    cleanup_confirmed: killed && joined,
                });
            }
        }
        let mut closed = 0;
        let decision = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> LoopExit {
            loop {
                if cancel.load(Ordering::Acquire) {
                    return LoopExit::Cancelled;
                }
                if started.elapsed() >= deadline {
                    return LoopExit::Timeout(accounting(h(&job)).map(|x| x.1).unwrap_or(0));
                }
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(Message::Data(data)) => match events.try_send(data) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                            return LoopExit::Backpressure
                        }
                    },
                    Ok(Message::Closed) => closed += 1,
                    Ok(Message::Error(message)) => return LoopExit::Pipe(message),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => closed = 2,
                }
                let count = match accounting(h(&job)) {
                    Ok(x) => x.0,
                    Err(e) => return LoopExit::System(format!("job accounting: {e:?}")),
                };
                if count == 0 && closed >= 2 {
                    let wait = WaitForSingleObject(child.process, 0);
                    if wait == WAIT_FAILED {
                        return LoopExit::System(format!(
                            "process wait: {:?}",
                            io::Error::last_os_error()
                        ));
                    }
                    if wait != WAIT_OBJECT_0 {
                        continue;
                    }
                    let mut code = 0;
                    if GetExitCodeProcess(child.process, &mut code) == 0 {
                        return LoopExit::System(format!(
                            "exit status: {:?}",
                            io::Error::last_os_error()
                        ));
                    }
                    return LoopExit::Success(code);
                }
            }
        }));
        let killed = if matches!(&decision, Ok(LoopExit::Success(_))) {
            true
        } else {
            terminate(h(&job))
        };
        drop(rx);
        let joined = join_bounded(threads);
        let cleaned = killed && joined;
        match decision {
            Ok(LoopExit::Success(code)) if cleaned => Ok(Outcome { code, active: 0 }),
            Ok(LoopExit::Success(_)) => Err(Failure::System {
                message: "pipe reader cleanup unconfirmed".into(),
                cleanup_confirmed: Some(false),
            }),
            Ok(LoopExit::Timeout(total_processes)) => Err(Failure::Timeout {
                cleanup_confirmed: cleaned,
                total_processes,
            }),
            Ok(LoopExit::Backpressure) => Err(Failure::Backpressure {
                cleanup_confirmed: cleaned,
            }),
            Ok(LoopExit::Cancelled) => Err(Failure::Cancelled {
                cleanup_confirmed: cleaned,
            }),
            Ok(LoopExit::Pipe(message)) => Err(Failure::Pipe {
                message,
                cleanup_confirmed: cleaned,
            }),
            Ok(LoopExit::System(message)) => Err(Failure::System {
                message,
                cleanup_confirmed: Some(cleaned),
            }),
            Err(_) => Err(Failure::Panic {
                cleanup_confirmed: cleaned,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pre_cancelled_does_not_launch() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let cancel = AtomicBool::new(true);
        let r = run(
            Path::new(r"C:\pg35-no-such-executable.exe"),
            &[],
            Duration::from_secs(1),
            tx,
            &cancel,
        );
        assert_eq!(
            r,
            Err(Failure::Cancelled {
                cleanup_confirmed: true
            })
        );
    }
}
