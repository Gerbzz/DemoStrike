//! PG80 scoped no-job video owner. Nonempty recording jobs are unavailable.
use serde_json::{json,Value};
use std::{sync::Mutex,thread,time::{Duration,Instant}};
struct State{pending:Vec<Value>,current:Option<Value>,paused:bool,stopped:bool}
pub struct OwnedVideoOwner{state:Mutex<State>}
impl OwnedVideoOwner{
 pub fn new()->Self{Self{state:Mutex::new(State{pending:vec![],current:None,paused:true,stopped:false})}}
 pub fn snapshot(&self)->Result<(Vec<Value>,bool),String>{let s=self.state.lock().map_err(|_|"video owner poisoned")?;if s.stopped{return Err("video owner stopped".into())}let mut videos=s.pending.clone();if let Some(v)=&s.current{videos.push(v.clone())}Ok((videos,s.paused))}
 pub fn request(&self,name:&str,payload:Option<Value>,emit:&mut impl FnMut(Value)->Result<(),String>)->Result<Value,String>{let mut s=self.state.lock().map_err(|_|"video owner poisoned")?;if s.stopped{return Err("video owner stopped".into())}match name{
 "add-video-to-queue"=>Err("nonempty video jobs are unsupported in owned fixture".into()),
 "pause-video-queue"=>{s.paused=true;emit(json!({"name":"video-queue-paused"}))?;Ok(Value::Null)},
 "resume-video-queue"=>{if s.paused{s.paused=false;emit(json!({"name":"video-queue-resumed"}))?}Ok(Value::Null)},
 "remove-videos-from-queue"=>{let ids=payload.as_ref().and_then(Value::as_array).ok_or("expected video ids")?;if ids.iter().any(|v|!v.is_string()){return Err("invalid video ids".into())}s.pending.retain(|v|!ids.contains(&v["id"]));if s.current.as_ref().is_some_and(|v|ids.contains(&v["id"])){s.current=None}emit(json!({"name":"videos-removed-from-queue","payload":ids}))?;Ok(Value::Null)},
 _=>Err("unsupported video request".into())}}
 pub fn stop(&self,timeout:Duration)->Result<(),String>{let deadline=Instant::now()+timeout;let mut s=loop{if Instant::now()>=deadline{return Err("video owner stop unconfirmed".into())}match self.state.try_lock(){Ok(g)=>break g,Err(std::sync::TryLockError::Poisoned(_))=>return Err("video owner poisoned".into()),Err(std::sync::TryLockError::WouldBlock)=>thread::sleep(Duration::from_millis(5))}};s.stopped=true;s.pending.clear();s.current=None;Ok(())}
}
#[cfg(test)] mod tests{use super::*;use serde_json::json;
 #[test]fn original_empty_video_owner_events_and_snapshot(){let o:Value=serde_json::from_str(include_str!("../tests/fixtures/pg80-original-video-empty-oracle.json")).unwrap();let owner=OwnedVideoOwner::new();let (v,p)=owner.snapshot().unwrap();assert_eq!(json!({"videos":v,"paused":p}),o["before"]);let mut events=Vec::new();let mut send=|v|{events.push(v);Ok(())};for n in ["resume-video-queue","resume-video-queue","pause-video-queue","pause-video-queue"]{owner.request(n,None,&mut send).unwrap();}owner.request("remove-videos-from-queue",Some(json!(["missing"])),&mut send).unwrap();let (v,p)=owner.snapshot().unwrap();assert_eq!(json!({"videos":v,"paused":p}),o["after"]);assert_eq!(json!(events),o["events"]);}
 #[test]fn nonempty_admission_rejects_and_stop_is_bounded(){let owner=OwnedVideoOwner::new();assert!(owner.request("add-video-to-queue",Some(json!({})),&mut |_|Ok(())).is_err());let g=owner.state.lock().unwrap();assert!(owner.stop(Duration::from_millis(30)).unwrap_err().contains("unconfirmed"));drop(g);owner.stop(Duration::from_secs(1)).unwrap();assert!(owner.snapshot().is_err());}
}
