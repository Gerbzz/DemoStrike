//! A bounded Windows process group for a user-started offline capture.
//! The job handle stays alive in the worker and closes on cancellation,
//! timeout, error, or application exit, killing every child in the job.
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct OwnedLaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
}

#[cfg(not(windows))]
pub fn run_owned(
    _launch: OwnedLaunch, _cancel: &AtomicBool, _timeout: Duration,
    _progress: impl FnMut(String),
) -> Result<(), String> {
    Err("offline capture is supported on Windows only".into())
}

#[cfg(not(windows))]
pub fn run_owned_with_monitor(
    _launch: OwnedLaunch, _cancel: &AtomicBool, _timeout: Duration,
    _label: &str, _progress: impl FnMut(String),
    _monitor: impl FnMut() -> Result<Option<String>, String>,
) -> Result<(), String> {
    Err("owned media QC is supported on Windows only".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    static OWNED_PROCESS_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn canceled_capture_does_not_start_a_process() {
        let canceled = AtomicBool::new(true);
        let missing = OwnedLaunch {
            program: PathBuf::from("missing-hlae.exe"),
            args: Vec::new(), current_dir: PathBuf::from("."),
            environment: Vec::new(),
        };
        assert!(run_owned(missing, &canceled, Duration::from_secs(60), |_| {})
            .unwrap_err().contains("canceled before launch"));
    }

    #[test]
    fn owned_job_harmless_exit_and_cancellation() {
        let _serial = OWNED_PROCESS_TEST.lock().unwrap();
        let windows = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows root"));
        let whoami = windows.join("System32/whoami.exe");
        let idle = AtomicBool::new(false);
        let harmless = OwnedLaunch {
            current_dir: whoami.parent().unwrap().to_path_buf(),
            program: whoami, args: Vec::new(), environment: Vec::new(),
        };
        run_owned(harmless, &idle, Duration::from_secs(10), |_| {}).unwrap();

        let powershell = windows.join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let signal = std::sync::Arc::clone(&stop);
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            signal.store(true, Ordering::Relaxed);
        });
        let sleeper = OwnedLaunch {
            current_dir: powershell.parent().unwrap().to_path_buf(),
            program: powershell,
            args: ["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 20"]
                .into_iter().map(OsString::from).collect(),
            environment: Vec::new(),
        };
        let started = std::time::Instant::now();
        let result = run_owned(sleeper, &stop, Duration::from_secs(5), |_| {});
        sender.join().unwrap();
        assert!(result.unwrap_err().contains("canceled"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn owned_monitor_failure_terminates_child_job() {
        let _serial = OWNED_PROCESS_TEST.lock().unwrap();
        let windows = PathBuf::from(std::env::var_os("SystemRoot").expect("Windows root"));
        let powershell = windows.join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let launch = OwnedLaunch {
            current_dir: powershell.parent().unwrap().to_path_buf(),
            program: powershell,
            args: ["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 20"]
                .into_iter().map(OsString::from).collect(),
            environment: Vec::new(),
        };
        let started = std::time::Instant::now();
        let result = run_owned_with_monitor(launch, &AtomicBool::new(false),
            Duration::from_secs(10), "Synthetic QC", |_| {},
            || Err("diagnostic limit reached".into()));
        let error = result.unwrap_err();
        assert!(error.contains("diagnostic limit reached"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

#[cfg(windows)]
pub fn run_owned(
    launch: OwnedLaunch, cancel: &AtomicBool, timeout: Duration,
    progress: impl FnMut(String),
) -> Result<(), String> {
    run_owned_with_monitor(launch, cancel, timeout, "Offline capture", progress,
        || Ok(None))
}

#[cfg(windows)]
pub fn run_owned_with_monitor(
    launch: OwnedLaunch, cancel: &AtomicBool, timeout: Duration,
    label: &str, mut progress: impl FnMut(String),
    mut monitor: impl FnMut() -> Result<Option<String>, String>,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CreateMutexW, CreateProcessW, GetExitCodeProcess, ReleaseMutex, ResumeThread, TerminateProcess,
        WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
        PROCESS_INFORMATION, STARTUPINFOW,
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this handle was returned by the Windows API and is owned here.
                unsafe { CloseHandle(self.0); }
            }
        }
    }
    struct CaptureMutex(Handle);
    impl Drop for CaptureMutex {
        fn drop(&mut self) {
            // SAFETY: this thread acquired the mutex below and still owns it.
            unsafe { ReleaseMutex(self.0.0); }
        }
    }
    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain([0]).collect()
    }
    fn quoted(value: &std::ffi::OsStr) -> Vec<u16> {
        let mut out = vec![b'"' as u16];
        let mut slashes = 0usize;
        for c in value.encode_wide() {
            if c == b'\\' as u16 { slashes += 1; continue; }
            if c == b'"' as u16 {
                out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            } else {
                out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            }
            slashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        out.push(b'"' as u16);
        out
    }
    if cancel.load(Ordering::Relaxed) { return Err("capture canceled before launch".into()); }
    if timeout.is_zero() || timeout > Duration::from_secs(30 * 60) {
        return Err("capture time budget must be between one second and 30 minutes".into());
    }
    if !launch.program.is_file() || !launch.current_dir.is_dir() {
        return Err("capture executable or working directory is missing".into());
    }
    let mutex_name = wide(std::ffi::OsStr::new("Local\\DemoStrikeOfflineCapture_v1"));
    // SAFETY: a null security descriptor uses the current process defaults.
    let mutex = Handle(unsafe { CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr()) });
    if mutex.0.is_null() { return Err(format!("cannot create capture lock: {}", std::io::Error::last_os_error())); }
    // A second window cannot launch another pilot between preflight and spawn.
    let acquired = unsafe { WaitForSingleObject(mutex.0, 0) };
    if acquired == WAIT_TIMEOUT {
        return Err("another DemoStrike offline capture is already active".into());
    }
    if acquired != WAIT_OBJECT_0 && acquired != WAIT_ABANDONED {
        return Err(format!("cannot acquire capture lock: {}", std::io::Error::last_os_error()));
    }
    let _capture_mutex = CaptureMutex(mutex);
    let program = wide(launch.program.as_os_str());
    let cwd = wide(launch.current_dir.as_os_str());
    let mut command_line = quoted(launch.program.as_os_str());
    for arg in &launch.args {
        command_line.push(b' ' as u16);
        command_line.extend(quoted(arg));
    }
    command_line.push(0);
    // Windows may expose per-drive pseudo variables whose names begin with
    // '='. The child has an explicit working directory and does not need them.
    let mut environment: Vec<_> = std::env::vars_os().filter(|(key, value)|
        !key.to_string_lossy().contains('=') && !key.to_string_lossy().contains('\0')
        && !value.to_string_lossy().contains('\0')).collect();
    for (key, value) in launch.environment {
        if key.to_string_lossy().contains('=') || key.to_string_lossy().contains('\0')
            || value.to_string_lossy().contains('\0') {
            return Err("capture environment override is invalid".into());
        }
        environment.retain(|(old, _)| !old.to_string_lossy().eq_ignore_ascii_case(&key.to_string_lossy()));
        environment.push((key, value));
    }
    environment.sort_by_key(|(key, _)| key.to_string_lossy().to_ascii_lowercase());
    let mut block: Vec<u16> = Vec::new();
    for (key, value) in environment {
        block.extend(key.encode_wide());
        block.push(b'=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() { block.push(0); }
    block.push(0);
    // SAFETY: null security attributes and name are valid; the returned handle
    // is immediately placed in an RAII wrapper.
    let job = Handle(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    if job.0.is_null() { return Err(format!("cannot create capture job: {}", std::io::Error::last_os_error())); }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: limits has the layout and byte size required for this information class.
    if unsafe { SetInformationJobObject(job.0, JobObjectExtendedLimitInformation,
        (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
        std::mem::size_of_val(&limits) as u32) } == 0 {
        return Err(format!("cannot set capture job limit: {}", std::io::Error::last_os_error()));
    }
    let mut startup = STARTUPINFOW::default();
    startup.cb = std::mem::size_of_val(&startup) as u32;
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: all wide buffers are NUL terminated, command_line is mutable,
    // and process/thread handles are closed below on every path.
    if unsafe { CreateProcessW(program.as_ptr(), command_line.as_mut_ptr(),
        std::ptr::null(), std::ptr::null(), 0,
        CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT, block.as_ptr().cast(),
        cwd.as_ptr(), &startup, &mut pi) } == 0 {
        return Err(format!("offline capture launch failed: {}", std::io::Error::last_os_error()));
    }
    let process = Handle(pi.hProcess);
    let thread = Handle(pi.hThread);
    // No game code runs before the process is assigned. An assignment failure
    // terminates the still-suspended launcher and refuses the capture.
    if unsafe { AssignProcessToJobObject(job.0, process.0) } == 0 {
        let error = std::io::Error::last_os_error();
        unsafe { TerminateProcess(process.0, 1); }
        return Err(format!("cannot own capture child process: {error}"));
    }
    if cancel.load(Ordering::Relaxed) {
        unsafe { TerminateJobObject(job.0, 1); }
        return Err("capture canceled before game start".into());
    }
    if unsafe { ResumeThread(thread.0) } == u32::MAX {
        let error = std::io::Error::last_os_error();
        unsafe { TerminateJobObject(job.0, 1); }
        return Err(format!("cannot start owned capture process: {error}"));
    }
    progress(format!("{label} running in an owned Windows job (PID {}).", pi.dwProcessId));
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            unsafe { TerminateJobObject(job.0, 1); }
            return Err("capture canceled; owned game processes were stopped".into());
        }
        if started.elapsed() >= timeout {
            unsafe { TerminateJobObject(job.0, 1); }
            return Err(format!("{label} exceeded its {} minute time budget; owned processes were stopped",
                timeout.as_secs() / 60));
        }
        match monitor() {
            Ok(Some(message)) => progress(message),
            Ok(None) => {},
            Err(error) => {
                unsafe { TerminateJobObject(job.0, 1); }
                return Err(format!("{label} stopped by output or disk monitor: {error}"));
            }
        }
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: accounting is a correctly sized output buffer for this job query.
        if unsafe { QueryInformationJobObject(job.0, JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            std::mem::size_of_val(&accounting) as u32, std::ptr::null_mut()) } == 0 {
            return Err(format!("cannot inspect owned capture processes: {}", std::io::Error::last_os_error()));
        }
        if accounting.ActiveProcesses == 0 {
            let mut launcher_code = 0_u32;
            if unsafe { GetExitCodeProcess(process.0, &mut launcher_code) } == 0 {
                return Err(format!("cannot read HLAE launcher exit code: {}", std::io::Error::last_os_error()));
            }
            if launcher_code != 0 {
                return Err(format!("{label} process exited with code {launcher_code}; inspect its diagnostics"));
            }
            progress(format!("{label} process exited. Outputs remain unreviewed until a person inspects motion, POV, and audio."));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
