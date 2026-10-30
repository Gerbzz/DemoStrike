//! CLI analysis input classification, with I/O and queue authority supplied by the caller.
use serde_json::{json,Value};
fn supported(game:&str,source:&str)->bool{match game{
 "CSGO"=>matches!(source,"challengermode"|"ebot"|"esea"|"esl"|"esportal"|"faceit"|"fastcup"|"5eplay"|"perfectworld"|"popflash"|"valve"),
 "CS2"=>matches!(source,"challengermode"|"ebot"|"esl"|"esplay"|"esportal"|"esportligaen"|"faceit"|"fastcup"|"5eplay"|"matchzy"|"perfectworld"|"pracc"|"renown"|"valve"),
 "CS2 LT"=>matches!(source,"challengermode"|"faceit"|"valve"),_=>false}}
pub fn prepare_cli(payload:&Value,checksums:&[String],mut resolve:impl FnMut(&str)->Result<Value,String>)->Result<(Value,Vec<Value>),String>{
 let paths=payload.get("demoPaths").and_then(Value::as_array).ok_or("demoPaths must be an array")?;
 let force=match payload.get("force"){None|Some(Value::Null)|Some(Value::Bool(false))=>false,Some(Value::Number(n))=>n.as_f64().is_some_and(|v|v!=0.0),Some(Value::String(v))=>!v.is_empty(),Some(_)=>true};
 let override_source=payload.get("source");
 let mut demos=Vec::new();let mut added=Vec::new();let mut skipped=Vec::new();let mut unsupported=Vec::new();
 for item in paths{let path=item.as_str().ok_or("demo path must be a string")?;let mut demo=resolve(path)?;if let Some(value)=override_source{let obj=demo.as_object_mut().ok_or("demo must be an object")?;obj.insert("source".into(),value.clone());}let game=demo["game"].as_str().ok_or("demo game missing")?;let source=demo.get("source").cloned().unwrap_or(Value::Null);
 if !matches!(game,"CSGO"|"CS2"|"CS2 LT"){return Err("Unknown demo game".into())}
 if !source.as_str().is_some_and(|v|supported(game,v)){unsupported.push(json!({"demoPath":path,"source":source,"reason":"source"}));continue}
 if demo["type"].as_str()==Some("POV")&&game!="CSGO"{unsupported.push(json!({"demoPath":path,"source":source,"reason":"pov"}));continue}
 let checksum=demo["checksum"].as_str().ok_or("demo checksum missing")?.to_owned();if !force&&checksums.contains(&checksum){skipped.push(json!(path));continue}if added.iter().any(|item:&Value|item["checksum"]==checksum){continue}
 added.push(json!({"checksum":checksum,"demoPath":path}));demos.push(demo);
 }
 Ok((json!({"addedDemos":added,"skippedDemoPaths":skipped,"unsupportedDemos":unsupported}),demos))
}

pub fn handle_cli(payload:&Value,checksums:impl FnOnce()->Result<Vec<String>,String>,resolve:impl FnMut(&str)->Result<Value,String>,enqueue:impl FnOnce(Vec<Value>)->Result<bool,String>)->Result<Value,String>{let known=checksums()?;let (mut result,demos)=prepare_cli(payload,&known,resolve)?;if !enqueue(demos)?{result["addedDemos"]=json!([])}Ok(result)}
