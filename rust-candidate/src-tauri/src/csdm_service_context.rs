use std::{sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
pub(super) struct ServiceContext<S,C>{pub(super) settings:Arc<S>,pub(super) settings_admission:Arc<Mutex<()>>,pub(super) init_admission:Mutex<()>,pub(super) connection:Mutex<Option<C>>,pub(super) stopping:AtomicBool,connection_label:&'static str,stopping_error:&'static str}
impl<S,C> ServiceContext<S,C>{
 pub(super) fn new(settings:Arc<S>,settings_admission:Arc<Mutex<()>>,connection_label:&'static str,stopping_error:&'static str)->Self{Self{settings,settings_admission,init_admission:Mutex::new(()),connection:Mutex::new(None),stopping:AtomicBool::new(false),connection_label,stopping_error}}
 pub(super) fn replace<Q,B,M,R>(&self,timeout:Duration,quiesce:Q,background:B,make:M,resume:R)->Result<(),String> where Q:FnOnce(Duration)->Result<(),String>,B:FnOnce(Duration)->Result<(),String>,M:FnOnce()->Result<C,String>,R:FnOnce(Duration)->Result<(),String>{
  let deadline=Instant::now()+timeout;let _admission=lock_until(&self.init_admission,deadline,"initialization admission")?;
  if self.stopping.load(Ordering::Acquire){return Err(self.stopping_error.into())}
  quiesce(remaining(deadline))?;background(remaining(deadline))?;
  let mut slot=lock_until(&self.connection,deadline,self.connection_label)?;*slot=None;*slot=Some(make()?);
  if let Err(error)=resume(remaining(deadline)){*slot=None;return Err(error)}Ok(())
 }
 pub(super) fn disconnect<Q,B>(&self,timeout:Duration,quiesce:Q,background:B)->Result<(),String> where Q:FnOnce(Duration)->Result<(),String>,B:FnOnce(Duration)->Result<(),String>{
  let deadline=Instant::now()+timeout;let _admission=lock_until(&self.init_admission,deadline,"initialization admission")?;
  if self.stopping.load(Ordering::Acquire){return Err(self.stopping_error.into())}
  quiesce(remaining(deadline))?;background(remaining(deadline))?;
  *lock_until(&self.connection,deadline,self.connection_label)?=None;Ok(())
 }
}
fn remaining(deadline:Instant)->Duration{deadline.saturating_duration_since(Instant::now())}
fn lock_until<'a,T>(mutex:&'a Mutex<T>,deadline:Instant,label:&str)->Result<std::sync::MutexGuard<'a,T>,String>{loop{if Instant::now()>=deadline{return Err(format!("{label} timed out; outcome unconfirmed"))}match mutex.try_lock(){Ok(g)=>return Ok(g),Err(std::sync::TryLockError::Poisoned(_))=>return Err(format!("{label} poisoned; outcome unconfirmed")),Err(std::sync::TryLockError::WouldBlock)=>std::thread::sleep(Duration::from_millis(5))}}}
#[cfg(test)] mod tests {
 use super::*;use std::cell::RefCell;
 #[test] fn replacement_quiesces_and_stops_before_releasing_old_client_and_resumes_after_new(){
  let c=ServiceContext::new(Arc::new(()),Arc::new(Mutex::new(())),"fixture connection","fixture stopping");*c.connection.lock().unwrap()=Some(5);let calls=RefCell::new(Vec::new());
  c.replace(Duration::from_secs(2),|_|{calls.borrow_mut().push("quiesce");Ok(())},|_|{calls.borrow_mut().push("background");Ok(())},||{calls.borrow_mut().push("connect");Ok(9)},|_|{calls.borrow_mut().push("resume");Ok(())}).unwrap();
  assert_eq!(*calls.borrow(),vec!["quiesce","background","connect","resume"]);assert_eq!(*c.connection.lock().unwrap(),Some(9));
 }
 #[test] fn failures_preserve_or_release_client_at_original_owned_boundary(){
  let c=ServiceContext::new(Arc::new(()),Arc::new(Mutex::new(())),"fixture connection","fixture stopping");*c.connection.lock().unwrap()=Some(5);
  assert!(c.disconnect(Duration::from_secs(2),|_|Err("quiesce".into()),|_|panic!("background must not run")).is_err());assert_eq!(*c.connection.lock().unwrap(),Some(5));
  assert!(c.replace(Duration::from_secs(2),|_|Ok(()),|_|Ok(()),||Ok(9),|_|Err("resume".into())).is_err());assert_eq!(*c.connection.lock().unwrap(),None);
 }
 #[test] fn drop_and_failure_order_matches_owned_connection_lifetime(){
  use std::sync::Mutex as M;
  struct Client(&'static str,Arc<M<Vec<&'static str>>>);impl Drop for Client{fn drop(&mut self){self.1.lock().unwrap().push(self.0)}}
  let log=Arc::new(M::new(Vec::new()));let c=ServiceContext::new(Arc::new(()),Arc::new(M::new(())),"fixture connection","fixture stopping");
  *c.connection.lock().unwrap()=Some(Client("drop-old",log.clone()));
  let l=log.clone();assert!(c.replace(Duration::from_secs(2),|_|Ok(()),|_|Ok(()),move||{l.lock().unwrap().push("make");Err("make failed".into())},|_|panic!("must not resume")).is_err());
  assert_eq!(*log.lock().unwrap(),vec!["drop-old","make"]);assert!(c.connection.lock().unwrap().is_none());
  let l=log.clone();assert!(c.replace(Duration::from_secs(2),|_|Ok(()),|_|Ok(()),move||Ok(Client("drop-new",l)),|_|Err("resume failed".into())).is_err());
  assert_eq!(*log.lock().unwrap(),vec!["drop-old","make","drop-new"]);
 }
 #[test] fn failed_background_drain_retains_connection_and_stopping_rejects_callbacks(){
  let c=ServiceContext::new(Arc::new(()),Arc::new(Mutex::new(())),"fixture connection","fixture stopping");*c.connection.lock().unwrap()=Some(5);
  assert_eq!(c.disconnect(Duration::from_secs(2),|_|Ok(()),|_|Err("background".into())),Err("background".into()));assert_eq!(*c.connection.lock().unwrap(),Some(5));
  c.stopping.store(true,Ordering::Release);assert_eq!(c.disconnect(Duration::from_secs(2),|_|panic!("stopping"),|_|panic!("stopping")),Err("fixture stopping".into()));
 }

}
