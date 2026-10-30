//! Coalescing single-driver owner. Caller supplies cancellation-aware work.
use std::{sync::{Arc,Mutex,mpsc::{self,SyncSender,TrySendError},atomic::{AtomicBool,Ordering}},thread::{self,JoinHandle},time::{Duration,Instant}};
#[derive(Clone)]pub struct Wake{tx:SyncSender<()>,stopping:Arc<AtomicBool>}
impl Wake{pub fn wake(&self)->Result<(),String>{if self.stopping.load(Ordering::Acquire){return Err("Analysis owner is stopping".into())}match self.tx.try_send(()){Ok(())|Err(TrySendError::Full(_))=>Ok(()),Err(TrySendError::Disconnected(_))=>Err("Analysis owner stopped".into())}}}
pub struct Owner{wake:Wake,worker:Option<JoinHandle<()>>,errors:Arc<Mutex<Vec<String>>>}
impl Owner{
 pub fn start(mut drive:impl FnMut(&AtomicBool)->Result<(),String>+Send+'static)->Self{let(tx,rx)=mpsc::sync_channel(1);let stop=Arc::new(AtomicBool::new(false));let thread_stop=stop.clone();let errors=Arc::new(Mutex::new(Vec::new()));let out=errors.clone();let worker=thread::spawn(move||{while rx.recv().is_ok(){if thread_stop.load(Ordering::Acquire){break}if let Err(e)=drive(&thread_stop){if let Ok(mut errors)=out.lock(){errors.push(e)}else{break}}}});Self{wake:Wake{tx,stopping:stop},worker:Some(worker),errors}}
 pub fn wake(&self)->Result<(),String>{self.wake.wake()}
 pub fn wake_handle(&self)->Wake{self.wake.clone()}
 pub fn is_stopped(&self)->bool{self.worker.is_none()}
 pub fn shutdown(&mut self,timeout:Duration)->Result<(),String>{self.wake.stopping.store(true,Ordering::Release);let _=self.wake.tx.try_send(());let deadline=Instant::now()+timeout;while self.worker.as_ref().is_some_and(|w|!w.is_finished())&&Instant::now()<deadline{thread::sleep(Duration::from_millis(10))}if self.worker.as_ref().is_some_and(|w|!w.is_finished()){return Err("Analysis owner shutdown unconfirmed".into())}if let Some(worker)=self.worker.take(){if worker.join().is_err(){self.errors.lock().map_err(|_|"Analysis owner error state unavailable")?.push("Analysis owner panicked".into())}}let errors=self.errors.lock().map_err(|_|"Analysis owner error state unavailable")?;if !errors.is_empty(){return Err(format!("Analysis owner failures: {}",errors.join("; ")))}Ok(())}
}

impl Drop for Owner{fn drop(&mut self){self.wake.stopping.store(true,Ordering::Release);let _=self.wake.tx.try_send(());}}
