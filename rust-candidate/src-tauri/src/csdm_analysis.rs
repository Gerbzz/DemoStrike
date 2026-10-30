//! Source-derived analysis ownership state. This state alone does not run an analyzer or emit events.
use serde_json::{json,Value};
#[derive(Default)]
pub struct Analyses { pending:Vec<Value>, current:Vec<u64>, workers:Vec<(u64,Value)>, owners:Vec<(String,Vec<String>)>, clients:Vec<(String,bool)>, next_id:u64, started:Vec<u64>, executing:Vec<u64> }
pub struct Claim { pub run_id:u64, pub analysis:Value }
fn same_worker_identity(left:&Value,right:&Value)->bool{let (Some(mut a),Some(mut b))=(left.as_object().cloned(),right.as_object().cloned())else{return false};for key in ["status","output","errorCode"]{a.remove(key);b.remove(key);}a==b}
impl Analyses {
 pub fn new()->Self{Self::default()}
 pub fn add(&mut self,demos:Vec<Value>,client:Option<&str>,positions:Option<bool>,mut now:impl FnMut()->String)->Result<Option<Value>,String>{
  for d in &demos {for k in ["checksum","filePath","mapName","source"]{if d.get(k).and_then(Value::as_str).is_none(){return Err(format!("Missing analysis demo {k}"))}}}
  let new:Vec<Value>=demos.iter().filter(|d|!self.pending.iter().any(|a|a["demoChecksum"]==d["checksum"])).cloned().collect();
  if let Some(c)=client {for d in &demos {if let Some((_,owners))=self.owners.iter_mut().find(|(key,_)|d["checksum"].as_str()==Some(key.as_str())){if !owners.iter().any(|o|o==c){owners.push(c.into())}}}}
  if new.is_empty(){return Ok(None)}
  let added:Vec<Value>=new.into_iter().map(|d|{let mut a=json!({"addedAt":now(),"status":"pending","demoChecksum":d["checksum"],"demoPath":d["filePath"],"mapName":d["mapName"],"source":d["source"],"output":""});if let Some(p)=positions{a["analyzePositions"]=json!(p)}a}).collect();
  self.pending.extend(added.iter().cloned());if let Some(c)=client {for a in &added {let key=a["demoChecksum"].as_str().unwrap().to_owned();if let Some((_,o))=self.owners.iter_mut().find(|(k,_)|*k==key){*o=vec![c.into()]}else{self.owners.push((key,vec![c.into()]))}}}
  Ok(Some(json!({"name":"demos-added-to-analyses","payload":added})))
 }
 pub fn connect_client(&mut self,id:&str)->Result<(),String>{if id.is_empty()||self.clients.iter().any(|(c,_)|c==id){return Err("Invalid or reused client identity".into())}self.clients.push((id.to_owned(),true));Ok(())}
 pub fn closed_clients(&self)->Vec<String>{self.clients.iter().filter(|(_,live)|!*live).map(|(id,_)|id.clone()).collect()}
 pub fn client_connected(&self,id:&str)->bool{self.clients.iter().any(|(c,live)|c==id&&*live)}
 pub fn mark_client_closed(&mut self,id:&str){if let Some((_,live))=self.clients.iter_mut().find(|(c,_)|c==id){*live=false}else if !id.is_empty(){self.clients.push((id.to_owned(),false))}}
 pub fn close_client(&mut self,id:&str)->Option<Value>{self.mark_client_closed(id);self.disconnect(id)}
 pub fn forget_client(&mut self,id:&str){self.clients.retain(|(c,live)|c!=id||*live)}
 pub fn cli_close_blocked(&self)->bool{let Some(item)=self.pending.first()else{return false};let Some(checksum)=item["demoChecksum"].as_str()else{return false};self.owners.iter().find(|(key,_)|key==checksum).is_some_and(|(_,owners)|owners.iter().all(|id|self.clients.iter().any(|(key,live)|key==id&&!*live)))}
 pub fn disconnect(&mut self,id:&str)->Option<Value>{let keys:Vec<String>=self.owners.iter_mut().filter_map(|(k,o)|{let n=o.len();o.retain(|v|v!=id);if n!=o.len()&&o.is_empty(){Some(k.clone())}else{None}}).collect();if keys.is_empty(){return None}let refs:Vec<&str>=keys.iter().map(String::as_str).collect();self.remove(&refs);Some(json!({"name":"demos-removed-from-analyses","payload":keys}))}
 pub fn remove(&mut self,keys:&[&str]){self.pending.retain(|a|!keys.contains(&a["demoChecksum"].as_str().unwrap()));self.owners.retain(|(k,_)|!keys.contains(&k.as_str()));}
 pub fn claim(&mut self,max:usize)->Option<Claim>{let live=self.started.len()+self.current.iter().filter(|id|!self.started.contains(id)).count();if live>=max.min(8)||self.pending.is_empty()||self.cli_close_blocked(){return None}let id=self.next_id;self.next_id=self.next_id.checked_add(1)?;let item=self.pending.remove(0);let checksum=item["demoChecksum"].as_str().unwrap();self.owners.retain(|(k,_)|k!=checksum);self.current.push(id);self.workers.push((id,item.clone()));Some(Claim{run_id:id,analysis:item})}
 pub fn set_running_status(&mut self,id:u64,status:&str)->Option<Value>{self.set_status_with_error(id,status,None)}
 pub fn set_status_with_error(&mut self,id:u64,status:&str,code:Option<i64>)->Option<Value>{let (_,item)=self.workers.iter_mut().find(|(run_id,_)|*run_id==id)?;item["status"]=json!(status);match code{Some(c)=>item["errorCode"]=json!(c),None=>{item.as_object_mut()?.remove("errorCode");}}Some(item.clone())}
 pub fn append_output(&mut self,id:u64,text:&str)->Option<Value>{let (_,item)=self.workers.iter_mut().find(|(run_id,_)|*run_id==id)?;let previous=item["output"].as_str()?.to_owned();item["output"]=json!(format!("{previous}{text}"));Some(item.clone())}
 pub fn finish(&mut self,id:u64){self.executing.retain(|v|*v!=id);self.started.retain(|v|*v!=id);let Some((_,item))=self.workers.iter().find(|(worker,_)|*worker==id) else{return};let checksum=item["demoChecksum"].clone();self.current.retain(|active|self.workers.iter().find(|(worker,_)|worker==active).is_some_and(|(_,a)|a["demoChecksum"]!=checksum));self.workers.retain(|(worker,_)|*worker!=id);}
 pub fn owns_claim(&self,claim:&Claim)->bool{self.current.contains(&claim.run_id)&&!self.started.contains(&claim.run_id)&&self.workers.iter().any(|(id,item)|*id==claim.run_id&&*item==claim.analysis)}
 pub fn owns_started_claim(&self,claim:&Claim)->bool{self.started.contains(&claim.run_id)&&self.workers.iter().any(|(id,item)|*id==claim.run_id&&same_worker_identity(item,&claim.analysis))}
 pub fn begin_execution(&mut self,claim:&Claim)->bool{if !self.owns_started_claim(claim)||self.executing.contains(&claim.run_id){return false}self.executing.push(claim.run_id);true}
 pub fn begin_claim(&mut self,claim:&Claim)->bool{if !self.owns_claim(claim){return false}self.started.push(claim.run_id);true}
 pub fn has_pending(&self)->bool{!self.pending.is_empty()}
 pub fn all(&self)->Vec<Value>{self.pending.iter().cloned().chain(self.current.iter().filter_map(|id|self.workers.iter().find(|(worker,_)|worker==id).map(|(_,a)|a.clone()))).collect()}
 pub fn clear(&mut self){self.pending.clear();self.current.clear();self.owners.clear();self.started.clear();self.executing.clear();}
}








#[cfg(test)]mod duplicate_liveness_tests{use super::*;#[test]fn still_started_after_other_same_checksum_finishes(){let mut s=Analyses::new();let d=json!({"checksum":"same","filePath":"C:\\owned\\a.dem","source":"ebot","mapName":"m"});s.add(vec![d.clone(),d],None,None,||"t".into()).unwrap();let a=s.claim(2).unwrap();let b=s.claim(2).unwrap();assert!(s.begin_claim(&a));assert!(s.begin_claim(&b));s.finish(a.run_id);assert!(s.all().is_empty());assert!(s.owns_started_claim(&b));assert!(s.append_output(b.run_id,"late").is_some());}#[test]fn running_worker_identity_survives_output_and_status(){let mut s=Analyses::new();let d=json!({"checksum":"x","filePath":"C:\\owned\\a.dem","source":"ebot","mapName":"m"});s.add(vec![d],None,None,||"t".into()).unwrap();let claim=s.claim(1).unwrap();assert!(s.begin_claim(&claim));s.set_running_status(claim.run_id,"analyzing").unwrap();s.append_output(claim.run_id,"progress").unwrap();assert!(s.owns_started_claim(&claim));let mut forged=Claim{run_id:claim.run_id,analysis:claim.analysis.clone()};forged.analysis["demoPath"]=json!("C:\\owned\\other.dem");assert!(!s.owns_started_claim(&forged));s.finish(claim.run_id);assert!(!s.owns_started_claim(&claim));}
#[test]fn worker_execution_is_claimed_exactly_once(){let mut s=Analyses::new();let d=json!({"checksum":"x","filePath":"C:\\owned\\a.dem","source":"ebot","mapName":"m"});s.add(vec![d],None,None,||"t".into()).unwrap();let c=s.claim(1).unwrap();assert!(s.begin_claim(&c));assert!(s.begin_execution(&c));assert!(!s.begin_execution(&c));s.finish(c.run_id);assert!(!s.begin_execution(&c));}
#[test]fn concurrent_replay_allows_one_execution(){use std::sync::{Arc,Barrier,Mutex};use std::thread;let mut s=Analyses::new();let d=json!({"checksum":"x","filePath":"a.dem","source":"ebot","mapName":"m"});s.add(vec![d],None,None,||"t".into()).unwrap();let c=s.claim(1).unwrap();assert!(s.begin_claim(&c));let shared=Arc::new(Mutex::new(s));let barrier=Arc::new(Barrier::new(3));let mut joins=Vec::new();for _ in 0..2{let a=shared.clone();let b=barrier.clone();let c=Claim{run_id:c.run_id,analysis:c.analysis.clone()};joins.push(thread::spawn(move||{b.wait();a.lock().unwrap().begin_execution(&c)}))}barrier.wait();let results:Vec<bool>=joins.into_iter().map(|j|j.join().unwrap()).collect();assert_eq!(results.iter().filter(|x|**x).count(),1);}
#[test]fn detached_duplicate_still_counts_against_live_limit(){let mut s=Analyses::new();let d=|c|json!({"checksum":c,"filePath":format!("{c}.dem"),"source":"ebot","mapName":"m"});s.add(vec![d("same"),d("same"),d("c"),d("d")],None,None,||"t".into()).unwrap();let a=s.claim(2).unwrap();let b=s.claim(2).unwrap();assert!(s.begin_claim(&a));assert!(s.begin_claim(&b));s.finish(a.run_id);let c=s.claim(2).unwrap();assert_eq!(c.analysis["demoChecksum"],"c");assert!(s.begin_claim(&c));assert!(s.claim(2).is_none());s.finish(b.run_id);assert_eq!(s.claim(2).unwrap().analysis["demoChecksum"],"d");}
}
