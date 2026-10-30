use std::path::Path;
#[derive(Debug, PartialEq, Eq)]
pub enum PrepareError {
    InvalidPath,
}
pub fn build_args(
    demo: &Path,
    output: &Path,
    source: &str,
    positions: bool,
) -> Result<Vec<String>, PrepareError> {
    let demo = demo.to_str().ok_or(PrepareError::InvalidPath)?;
    let output = output.to_str().ok_or(PrepareError::InvalidPath)?;
    let mut args = vec![
        format!("-demo-path={demo}"),
        format!("-output={output}"),
        "-format=csdm".into(),
    ];
    if !source.is_empty() {
        args.push(format!("-source={source}"))
    }
    if positions {
        args.push("-positions=true".into())
    }
    Ok(args)
}

#[cfg(windows)]
use super::csdm_windows_job as job;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};
#[derive(Debug, PartialEq, Eq)]
pub struct TextEvent {
    pub stderr: bool,
    pub text: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum AnalyzerError {
    InvalidArguments,
    MissingDemo,
    InvalidOutput,
    InvalidDeadline,
    Spawn(String),
    #[cfg(windows)]
    Job(job::Failure),
    JobThreadPanic {
        cleanup_confirmed: Option<bool>,
    },
    UnsupportedPlatform,
    CanceledOutput {
        cleanup_confirmed: Option<bool>,
    },
    Corrupted,
    ProcessFailure(u32),
}
pub struct Request {
    pub executable: PathBuf,
    pub demo: PathBuf,
    pub output: PathBuf,
    pub source: String,
    pub positions: bool,
    pub deadline: Duration,
}
#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
}
impl Decoder {
    fn push(&mut self, b: &[u8]) -> String {
        self.pending.extend_from_slice(b);
        let mut out = String::new();
        let mut pos = 0;
        while pos < self.pending.len() {
            match std::str::from_utf8(&self.pending[pos..]) {
                Ok(s) => {
                    out.push_str(s);
                    pos = self.pending.len()
                }
                Err(e) => {
                    let n = e.valid_up_to();
                    out.push_str(
                        std::str::from_utf8(&self.pending[pos..pos + n]).expect("validated UTF-8"),
                    );
                    pos += n;
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{fffd}');
                            pos += n
                        }
                        None => break,
                    }
                }
            }
        }
        self.pending.drain(..pos);
        out
    }
    fn finish(&mut self) -> String {
        let s = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        s
    }
}
#[derive(Default)]
struct Marker {
    tail: Vec<u8>,
    found: bool,
}
impl Marker {
    fn push(&mut self, text: &str) {
        const NEEDLE: &[u8] = b"ErrUnexpectedEndOfDemo";
        self.tail.extend_from_slice(text.as_bytes());
        self.found |= self.tail.windows(NEEDLE.len()).any(|w| w == NEEDLE);
        let keep = NEEDLE.len() - 1;
        if self.tail.len() > keep {
            self.tail.drain(..self.tail.len() - keep);
        }
    }
}
#[cfg(windows)]
fn cleanup_of(r: &Result<job::Outcome, job::Failure>) -> Option<bool> {
    match r {
        Ok(_) => Some(true),
        Err(
            job::Failure::Timeout {
                cleanup_confirmed, ..
            }
            | job::Failure::Backpressure { cleanup_confirmed }
            | job::Failure::Cancelled { cleanup_confirmed }
            | job::Failure::Pipe {
                cleanup_confirmed, ..
            }
            | job::Failure::Panic { cleanup_confirmed },
        ) => Some(*cleanup_confirmed),
        Err(job::Failure::System {
            cleanup_confirmed, ..
        }) => *cleanup_confirmed,
    }
}
/// Runs only the supplied executable and paths. Caller must establish executable and output ownership,
/// and continuously drain the bounded event receiver. No queue/database readiness is implied.
#[cfg(windows)]
pub fn analyze(req: &Request, events: SyncSender<TextEvent>) -> Result<(), AnalyzerError> {
    if !req.demo.exists() {
        return Err(AnalyzerError::MissingDemo);
    }
    if !req.output.is_dir() {
        return Err(AnalyzerError::InvalidOutput);
    }
    if req.deadline.is_zero() {
        return Err(AnalyzerError::InvalidDeadline);
    }
    let args = build_args(&req.demo, &req.output, &req.source, req.positions)
        .map_err(|_| AnalyzerError::InvalidArguments)?;
    let cancel = AtomicBool::new(false);
    thread::scope(|scope| {
        let (tx, rx) = mpsc::sync_channel(16);
        let cancel_ref = &cancel;
        let args_ref = &args;
        let worker = thread::Builder::new()
            .spawn_scoped(scope, move || {
                let refs: Vec<&str> = args_ref.iter().map(String::as_str).collect();
                job::run(&req.executable, &refs, req.deadline, tx, cancel_ref)
            })
            .map_err(|e| AnalyzerError::Spawn(e.to_string()))?;
        let (mut stdout, mut stderr) = (Decoder::default(), Decoder::default());
        let mut marker = Marker::default();
        let mut sink_failed = false;
        for item in rx {
            let text = if item.stderr {
                stderr.push(&item.bytes)
            } else {
                stdout.push(&item.bytes)
            };
            if item.stderr {
                marker.push(&text)
            }
            if !text.is_empty() && !sink_failed {
                match events.try_send(TextEvent {
                    stderr: item.stderr,
                    text,
                }) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                        sink_failed = true;
                        cancel.store(true, Ordering::Release)
                    }
                }
            }
        }
        for (is_err, text) in [(false, stdout.finish()), (true, stderr.finish())] {
            if is_err {
                marker.push(&text)
            }
            if !text.is_empty() && !sink_failed {
                match events.try_send(TextEvent {
                    stderr: is_err,
                    text,
                }) {
                    Ok(()) => {}
                    Err(_) => {
                        sink_failed = true;
                        cancel.store(true, Ordering::Release)
                    }
                }
            }
        }
        let result = worker.join().map_err(|_| AnalyzerError::JobThreadPanic {
            cleanup_confirmed: None,
        })?;
        if sink_failed {
            return Err(AnalyzerError::CanceledOutput {
                cleanup_confirmed: cleanup_of(&result),
            });
        }
        let outcome = result.map_err(AnalyzerError::Job)?;
        if marker.found {
            return Err(AnalyzerError::Corrupted);
        }
        if outcome.code != 0 {
            return Err(AnalyzerError::ProcessFailure(outcome.code));
        }
        Ok(())
    })
}

#[cfg(not(windows))]
pub fn analyze(_req: &Request, _events: SyncSender<TextEvent>) -> Result<(), AnalyzerError> {
    Err(AnalyzerError::UnsupportedPlatform)
}
