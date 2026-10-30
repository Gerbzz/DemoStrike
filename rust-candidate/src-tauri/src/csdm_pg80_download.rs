//! Scoped owned no-network download owner. Nonempty jobs are explicitly unsupported.
//! Ordering and empty/abort behavior derive from original download-queue.ts.
use serde_json::Value;
use std::{sync::Mutex,time::{Duration,Instant},thread};
#[derive(Default)] struct State{pending:Vec<Value>,current:Option<Value>,stopped:bool}
pub struct OwnedDownloadOwner{state:Mutex<State>}
impl OwnedDownloadOwner{
 pub fn new()->Self{Self{state:Mutex::new(State::default())}}
 pub fn snapshot(&self)->Result<Vec<Value>,String>{let state=self.state.lock().map_err(|_|"owned download owner poisoned")?;if state.stopped{return Err("owned download owner stopped".into())}let mut out=state.pending.clone();if let Some(item)=&state.current{out.push(item.clone())}Ok(out)}
 pub fn request(&self,name:&str,payload:Option<Value>)->Result<Value,String>{let mut state=self.state.lock().map_err(|_|"owned download owner poisoned")?;if state.stopped{return Err("owned download owner stopped".into())}match name{
  "add-downloads"=>{let values=payload.as_ref().and_then(Value::as_array).ok_or("expected downloads array")?;if !values.is_empty(){return Err("nonempty download jobs are not supported by the owned fixture".into())}Ok(Value::Null)},
  "add-download"|"add-download-from-share-code"=>Err("nonempty download jobs are not supported by the owned fixture".into()),
  "abort-download"=>{let id=payload.as_ref().and_then(Value::as_str).ok_or("expected match id")?;state.pending.retain(|x|x["matchId"].as_str()!=Some(id));if state.current.as_ref().is_some_and(|x|x["matchId"].as_str()==Some(id)){state.current=None}Ok(Value::Null)},
  "abort-downloads"=>{state.pending.clear();state.current=None;Ok(Value::Null)},
  _=>Err("unsupported owned download request".into())}}
 pub fn stop(&self,timeout:Duration)->Result<(),String>{let deadline=Instant::now()+timeout;let mut state=loop{if Instant::now()>=deadline{return Err("owned download owner stop unconfirmed".into())}match self.state.try_lock(){Ok(g)=>break g,Err(std::sync::TryLockError::Poisoned(_))=>return Err("owned download owner poisoned".into()),Err(std::sync::TryLockError::WouldBlock)=>thread::sleep(Duration::from_millis(5))}};state.stopped=true;state.pending.clear();state.current=None;Ok(())}
}
#[cfg(test)]mod tests{use super::*;use serde_json::json;
 #[test]fn source_original_empty_queue_and_cancellation_oracle(){let oracle:Value=serde_json::from_str(include_str!("../tests/fixtures/pg80-original-download-empty-oracle.json")).unwrap();let owner=OwnedDownloadOwner::new();assert_eq!(json!(owner.snapshot().unwrap()),oracle["before"]);assert_eq!(owner.request("add-downloads",Some(json!([]))).unwrap(),Value::Null);assert_eq!(oracle["empty"],json!([]));assert_eq!(owner.request("abort-download",Some(json!("missing"))).unwrap(),Value::Null);assert_eq!(owner.request("abort-downloads",None).unwrap(),Value::Null);assert_eq!(json!(owner.snapshot().unwrap()),oracle["after"]);assert_eq!(oracle["has"],false);assert_eq!(oracle["pushes"],json!([]));}
 #[test]fn bounded_stop_retries_after_contention(){let o=OwnedDownloadOwner::new();let held=o.state.lock().unwrap();let started=Instant::now();assert!(o.stop(Duration::from_millis(40)).unwrap_err().contains("unconfirmed"));assert!(started.elapsed()<Duration::from_secs(1));drop(held);o.stop(Duration::from_secs(1)).unwrap();}
 #[test]fn nonempty_paths_reject_without_changing_owner(){let o=OwnedDownloadOwner::new();for (name,p) in [("add-download",json!({"matchId":"x"})),("add-downloads",json!([{"matchId":"x"}])),("add-download-from-share-code",json!("x"))]{assert!(o.request(name,Some(p)).unwrap_err().contains("not supported"));assert!(o.snapshot().unwrap().is_empty())}o.stop(Duration::from_secs(1)).unwrap();assert!(o.snapshot().is_err());assert!(o.request("add-downloads",Some(json!([]))).is_err());}
}

