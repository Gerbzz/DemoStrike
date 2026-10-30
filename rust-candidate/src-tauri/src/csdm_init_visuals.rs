//! Original CSDM map/camera/tag readers, currently authorized for task-owned fixture roots only.
use serde_json::{json,Value};
use std::path::{Path,PathBuf};
#[derive(Clone)]
pub struct AssetRoots { pub user_images:PathBuf,pub bundled_images:PathBuf,pub fixture_scope:Option<PathBuf> }
fn first_existing(paths:&[PathBuf],scope:Option<&Path>)->Result<Option<PathBuf>,String> {
    for path in paths {
        if let Some(scope)=scope {if !lexical_normalize(path).starts_with(lexical_normalize(scope)){return Err("Asset path escapes the authorized fixture scope".into());}}
        if let Some(root)=scope {
            for a in path.ancestors() {
                if !a.starts_with(root) { break; }
                match std::fs::symlink_metadata(a) {
                    Ok(m)=>{ if m.file_type().is_symlink(){return Err("Fixture asset path contains symlink".into());}
                        #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if m.file_attributes() & 0x400 != 0 {return Err("Fixture asset path contains reparse point".into());}}
                    },
                    Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
                    Err(e)=>return Err(format!("Cannot inspect fixture asset path: {e}")),
                }
            }
        }
        if path.exists(){return Ok(Some(path.clone()));}
    }
    Ok(None)
}
fn path_string(p:&Path)->String {
    let value=p.to_string_lossy().into_owned();
    #[cfg(windows)] { value.replace('/', "\\") }
    #[cfg(not(windows))] { value }
}
fn game_dir(game:&str)->&str {if game=="CSGO"{"csgo"}else{"cs2"}}
fn js_number(v:f64)->Value { if v.is_finite() && v.fract()==0.0 && v.abs()<=9_007_199_254_740_991.0 {json!(v as i64)} else {json!(v)} }
fn radar_width(path:&Path)->Result<u32,String> {let b=std::fs::read(path).map_err(|e|e.to_string())?;if b.get(..8)!=Some(&[137,80,78,71,13,10,26,10]){return Err("Invalid PNG file".into())};if b.len()<24{let offset=if b.len()<20{16}else{20};return Err(format!("The value of \"offset\" is out of range. It must be >= 0 and <= {}. Received {offset}",b.len()-4))}Ok(u32::from_be_bytes(b[16..20].try_into().unwrap()))}
fn lexical_normalize(path:&Path)->PathBuf {
    use std::path::Component;
    let mut out=PathBuf::new();
    for component in path.components() {match component {
        Component::CurDir=>{},
        Component::ParentDir=>{if matches!(out.components().next_back(),Some(Component::Normal(_))){out.pop();}else if !out.has_root(){out.push("..");}},
        other=>out.push(other.as_os_str()),
    }}
    out
}
fn node_child(base:&Path,file:&str)->PathBuf {
    // Node path.join concatenates before normalization, even for rooted later parts.
    lexical_normalize(Path::new(&format!("{}{}{}",base.display(),std::path::MAIN_SEPARATOR,file)))
}
pub fn map_value(row:&Value,roots:&AssetRoots)->Result<Value,String> {
 let get=|k:&str|row.get(k).ok_or_else(||format!("Missing map field {k}"));let name=get("name")?.as_str().ok_or("Invalid map name")?;let game=get("game")?.as_str().ok_or("Invalid map game")?;let id=get("id")?.as_str().ok_or("Invalid map id")?;
 let base=|r:&Path,kind:&str,file:&str|node_child(&r.join("maps").join(game_dir(game)).join(kind),file);
 let choose=|kind:&str,file:&str|first_existing(&[base(&roots.user_images,kind,file),base(&roots.bundled_images,kind,file)],roots.fixture_scope.as_deref());
 let radar=choose("radars",&format!("{name}.png"))?;let lower=choose("radars",&format!("{name}_lower.png"))?;let thumb=choose("thumbnails",&format!("{name}.png"))?;
 let width=match &radar{Some(p)=>radar_width(p)?,None=>1024};let mut out=json!({"id":id,"name":name,"game":game,"posX":get("position_x")?,"posY":get("position_y")?,"thresholdZ":get("threshold_z")?,"scale":get("scale")?,"radarSize":width});
 for(k,p) in [("radarFilePath",radar),("lowerRadarFilePath",lower),("thumbnailFilePath",thumb)]{if let Some(p)=p{out[k]=json!(path_string(&p));}}
 Ok(out)
}
pub fn camera_value(row:&Value,roots:&AssetRoots)->Result<Value,String> {
 let get=|k:&str|row.get(k).ok_or_else(||format!("Missing camera field {k}"));let id=get("id")?.as_str().ok_or("Invalid camera id")?;
 let path=first_existing(&[roots.user_images.join("cameras").join(format!("{id}.png")),roots.user_images.join("cameras").join(format!("{id}.jpg")),roots.bundled_images.join("cameras").join(format!("{id}.png"))],roots.fixture_scope.as_deref())?;
 let num=|k:&str|->Result<Value,String>{Ok(js_number(get(k)?.as_f64().ok_or_else(||format!("Invalid camera {k}"))?))};
 Ok(json!({"id":id,"name":get("name")?,"mapName":get("map_name")?,"game":get("game")?,"x":num("x")?,"y":num("y")?,"z":num("z")?,"yaw":num("yaw")?,"pitch":num("pitch")?,"comment":get("comment")?,"color":get("color")?,"imagePath":path.map(|p|path_string(&p))}))
}
pub fn tag_value(row:&Value)->Result<Value,String> {let id=row.get("id").and_then(Value::as_str).ok_or("Invalid tag id")?;Ok(json!({"id":id,"name":row.get("name").ok_or("Missing tag name")?,"color":row.get("color").ok_or("Missing tag color")?}))}
pub fn postgres_maps(client:&mut postgres::Client,roots:&AssetRoots)->Result<Vec<Value>,String> {
 let rows=client.query("SELECT id::text AS id,name,game,position_x,position_y,threshold_z,scale::text AS scale FROM maps ORDER BY name",&[]).map_err(|e|e.to_string())?;
 rows.into_iter().map(|row|{let scale=row.try_get::<_,String>("scale").map_err(|e|e.to_string())?.parse::<f64>().map_err(|e|e.to_string())?;let get=||->Result<Value,postgres::Error>{Ok(json!({"id":row.try_get::<_,String>("id")?,"name":row.try_get::<_,String>("name")?,"game":row.try_get::<_,String>("game")?,"position_x":row.try_get::<_,i32>("position_x")?,"position_y":row.try_get::<_,i32>("position_y")?,"threshold_z":row.try_get::<_,i32>("threshold_z")?,"scale":scale}))};map_value(&get().map_err(|e|e.to_string())?,roots)}).collect()
}
pub fn postgres_cameras(client:&mut postgres::Client,roots:&AssetRoots)->Result<Vec<Value>,String> {
 let rows=client.query("SELECT id::text AS id,name,map_name,game,x,y,z,yaw,pitch,comment,color FROM cameras ORDER BY name",&[]).map_err(|e|e.to_string())?;
 rows.into_iter().map(|row|{let get=||->Result<Value,postgres::Error>{Ok(json!({"id":row.try_get::<_,String>("id")?,"name":row.try_get::<_,String>("name")?,"map_name":row.try_get::<_,String>("map_name")?,"game":row.try_get::<_,String>("game")?,"x":row.try_get::<_,f64>("x")?,"y":row.try_get::<_,f64>("y")?,"z":row.try_get::<_,f64>("z")?,"yaw":row.try_get::<_,f64>("yaw")?,"pitch":row.try_get::<_,f64>("pitch")?,"comment":row.try_get::<_,String>("comment")?,"color":row.try_get::<_,String>("color")?}))};camera_value(&get().map_err(|e|e.to_string())?,roots)}).collect()
}
pub fn postgres_tags(client:&mut postgres::Client)->Result<Vec<Value>,String> {
 let rows=client.query("SELECT id::text AS id,name,color FROM tags ORDER BY name ASC",&[]).map_err(|e|e.to_string())?;
 rows.into_iter().map(|row|{let get=||->Result<Value,postgres::Error>{Ok(json!({"id":row.try_get::<_,String>("id")?,"name":row.try_get::<_,String>("name")?,"color":row.try_get::<_,String>("color")?}))};tag_value(&get().map_err(|e|e.to_string())?)}).collect()
}
#[cfg(test)] mod tests {
use super::*;
#[test]
#[ignore = "requires explicitly provisioned PG13 owned original-schema fixtures"]
fn original_postgres_readers_match_source_oracle() {
 assert_eq!(std::env::var("PG13_MAP_TRIAL").as_deref(),Ok("1"));let pid=std::env::var("PG13_EXPECTED_POSTGRES_PID").unwrap();let data=r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg8-postgres-cluster";let text=std::fs::read_to_string(Path::new(data).join("postmaster.pid")).unwrap();let l:Vec<_>=text.lines().collect();assert!(l.len()>=8 && l[0]==pid && l[3]=="46603" && l[7].trim()=="ready");assert_eq!(l[1].replace('\\',"/").to_lowercase(),data.replace('\\',"/").to_lowercase());
 let oracle:Value=serde_json::from_slice(&std::fs::read(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg13-maps-original-oracle.json").unwrap()).unwrap();let root=PathBuf::from(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg12-mapping-fixtures-v3");let roots=AssetRoots{user_images:root.join("user/images"),bundled_images:root.join("bundled/images"),fixture_scope:Some(root.clone())};
 fn owned(db:&str)->postgres::Client {assert!(matches!(db,"csdm_pg13_maps_ok_20261029"|"csdm_pg13_maps_empty_20261029"|"csdm_pg13_maps_missing_20261029"));let mut cfg=postgres::Config::new();cfg.host("127.0.0.1").port(46603).user("csdm_test").password("").dbname(db).connect_timeout(std::time::Duration::from_secs(3));let mut c=cfg.connect(postgres::NoTls).unwrap();let d:String=c.query_one("SHOW data_directory",&[]).unwrap().try_get(0).unwrap();assert_eq!(d.replace('\\',"/").to_lowercase(),"C:/REDACTED/documents/codex/2026-10-27/task-6/pg8-postgres-cluster");c.batch_execute("SET default_transaction_read_only=on").unwrap();c}
 let mut empty=owned("csdm_pg13_maps_empty_20261029");assert_eq!(postgres_maps(&mut empty,&roots).unwrap(),Vec::<Value>::new());assert_eq!(postgres_cameras(&mut empty,&roots).unwrap(),Vec::<Value>::new());assert_eq!(postgres_tags(&mut empty).unwrap(),Vec::<Value>::new());
 let mut missing=owned("csdm_pg13_maps_missing_20261029");assert!(postgres_maps(&mut missing,&roots).is_err());assert!(postgres_cameras(&mut missing,&roots).is_err());assert!(postgres_tags(&mut missing).is_err());for k in ["maps","cameras","tags"]{assert_eq!(oracle["missing"][k],"42P01")}
 let mut ok=owned("csdm_pg13_maps_ok_20261029");assert_eq!(postgres_maps(&mut ok,&roots).unwrap(),serde_json::from_value::<Vec<Value>>(oracle["rows"]["maps"].clone()).unwrap());assert_eq!(postgres_cameras(&mut ok,&roots).unwrap(),serde_json::from_value::<Vec<Value>>(oracle["rows"]["cameras"].clone()).unwrap());assert_eq!(postgres_tags(&mut ok).unwrap(),serde_json::from_value::<Vec<Value>>(oracle["rows"]["tags"].clone()).unwrap());
}
}
