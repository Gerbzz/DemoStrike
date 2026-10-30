pub fn validate(name: &str, color: &str) -> Result<(), u32> {
    let len = name.encode_utf16().count();
    if len == 0 { return Err(3001); }
    if len > 40 { return Err(3002); }
    let upper = color.to_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(b)) { return Err(3003); }
    Ok(())
}
/// Operates on the caller's PostgreSQL client. The caller owns connection admission,
/// identity, and shutdown; no fixture or user-profile policy is embedded here.
pub fn request(client: &mut postgres::Client, payload: &serde_json::Value, update: bool) -> Result<serde_json::Value, u32> {
    let name = payload.get("name").and_then(serde_json::Value::as_str).ok_or(1u32)?;
    let color = payload.get("color").and_then(serde_json::Value::as_str).ok_or(1u32)?;
    validate(name, color)?;
    let map_error = |e: postgres::Error| {
        if e.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION) { 3000u32 }
        else { eprintln!("Tag database operation failed; commit outcome may be unknown: {e:?}"); 1u32 }
    };
    if update {
        let id = payload.get("id").and_then(serde_json::Value::as_str).ok_or(1u32)?;
        client.execute("UPDATE tags SET id=$1::text::bigint,name=$2,color=$3 WHERE id=$1::text::bigint", &[&id, &name, &color]).map_err(map_error)?;
        Ok(serde_json::Value::Null)
    } else {
        let row = client.query_one("INSERT INTO tags (name,color) VALUES ($1,$2) RETURNING id,name,color", &[&name, &color]).map_err(map_error)?;
        let id: i64 = row.try_get(0).map_err(|e| { eprintln!("Tag insert committed but returned row could not be decoded: {e:?}"); 1u32 })?;
        let name: String = row.try_get(1).map_err(|e| { eprintln!("Tag insert committed but returned row could not be decoded: {e:?}"); 1u32 })?;
        let color: String = row.try_get(2).map_err(|e| { eprintln!("Tag insert committed but returned row could not be decoded: {e:?}"); 1u32 })?;
        Ok(serde_json::json!({"id":id.to_string(),"name":name,"color":color}))
    }
}
/// Mirrors removeTagIdFromSettings for the three original settings branches.
pub fn cleanup_tag_settings(settings: &mut serde_json::Value, id: &str) -> Result<bool, String> {
    let mut changed = false;
    for key in ["demos", "matches", "playerProfile"] {
        let branch = settings.get_mut(key).and_then(serde_json::Value::as_object_mut).ok_or_else(|| format!("Invalid settings branch: {key}"))?;
        if let Some(serde_json::Value::Array(ids)) = branch.get_mut("tagIds") {
            let old_len = ids.len();
            ids.retain(|value| value.as_str() != Some(id));
            changed |= ids.len() != old_len;
        }
    }
    Ok(changed)
}
/// Database deletion precedes settings cleanup; a later error cannot undo the delete.
pub fn delete_flow<D,S,E>(delete:D,settings:S,emit:E)->Result<(),String> where D:FnOnce()->Result<(),String>,S:FnOnce()->Result<Option<serde_json::Value>,String>,E:FnOnce(serde_json::Value)->Result<(),String> {
    delete()?;
    if let Some(snapshot)=settings()? { emit(snapshot)?; }
    Ok(())
}
/// The original schema owns checksum relationship cleanup through its delete trigger.
pub fn delete(client:&mut postgres::Client,id:&str)->Result<(),String> {
    client.execute("DELETE FROM tags WHERE id=$1::text::bigint", &[&id]).map(|_|()).map_err(|e|format!("Tag database delete failed; commit outcome may be unknown: {e:?}"))
}
#[derive(Debug,PartialEq)]
pub(super) struct ChecksumPlan { checksums:Vec<String>, tag_ids:Vec<String> }
pub(super) fn checksum_plan(payload:&serde_json::Value)->Result<Option<ChecksumPlan>,String>{
 let tags=payload.get("tagIds").and_then(serde_json::Value::as_array).ok_or("Invalid tag IDs")?;
 let mut unique=Vec::new();let mut seen=std::collections::HashSet::new();for tag in tags {let id=tag.as_str().ok_or("Invalid tag ID")?;if seen.insert(id){unique.push(id.to_owned())}}
 if unique.is_empty(){return Ok(None)}
 let checksums=payload.get("checksums").and_then(serde_json::Value::as_array).ok_or("Invalid checksums")?.iter().map(|v|v.as_str().map(str::to_owned).ok_or("Invalid checksum".to_string())).collect::<Result<Vec<_>,_>>()?;
 if checksums.is_empty(){return Err("Original empty checksum insert is invalid".into())}Ok(Some(ChecksumPlan{checksums,tag_ids:unique}))
}
/// Replaces links as one transaction; a PostgreSQL failure rolls back prior deletes.
pub(super) fn update_checksum_tags(client:&mut postgres::Client,plan:ChecksumPlan)->Result<(),String>{
 use postgres::types::ToSql;
 let pairs=plan.checksums.len().checked_mul(plan.tag_ids.len()).ok_or("Checksum tag count overflow")?;
 if pairs>32767 {return Err("Checksum tag parameter limit exceeded".into())}
 let mut sql=String::from("INSERT INTO checksum_tags (checksum, tag_id) VALUES ");let mut params:Vec<&(dyn ToSql+Sync)>=Vec::with_capacity(pairs*2);
 for checksum in &plan.checksums {for id in &plan.tag_ids {if !params.is_empty(){sql.push(',')}let first=params.len()+1;sql.push_str(&format!("(${},${}::text::bigint)",first,first+1));params.push(checksum);params.push(id);}}
 sql.push_str(" ON CONFLICT (checksum, tag_id) DO NOTHING");
 let mut tx=client.transaction().map_err(|e|format!("Checksum tags transaction failed: {e}"))?;
 tx.execute("DELETE FROM checksum_tags WHERE checksum = ANY($1::text[]) AND tag_id NOT IN (SELECT value::bigint FROM unnest($2::text[]) AS value)",&[&plan.checksums,&plan.tag_ids]).map_err(|e|format!("Checksum tag delete failed: {e}"))?;
 tx.execute(&sql,&params).map_err(|e|format!("Checksum tag insert failed: {e}"))?;
 tx.commit().map_err(|e|format!("Checksum tag commit outcome may be unknown: {e}"))
}
/// Original players-tags payload: unique tag IDs, but preserve repeated Steam IDs.
#[derive(Debug,PartialEq)]
pub(super) struct PlayerPlan { steam_ids:Vec<String>, tag_ids:Vec<String> }
pub(super) fn player_plan(payload:&serde_json::Value)->Result<Option<PlayerPlan>,String>{
 let tags=payload.get("tagIds").and_then(serde_json::Value::as_array).ok_or("Invalid tag IDs")?;
 let mut unique=Vec::new();let mut seen=std::collections::HashSet::new();for tag in tags {let id=tag.as_str().ok_or("Invalid tag ID")?;if seen.insert(id){unique.push(id.to_owned())}}
 if unique.is_empty(){return Ok(None)}
 let steam_ids=payload.get("steamIds").and_then(serde_json::Value::as_array).ok_or("Invalid Steam IDs")?.iter().map(|v|v.as_str().map(str::to_owned).ok_or("Invalid Steam ID".to_string())).collect::<Result<Vec<_>,_>>()?;
 Ok(Some(PlayerPlan{steam_ids,tag_ids:unique}))
}
/// SQL update and insertion share the original transaction; failed commits remain unknown.
pub(super) fn update_player_tags(client:&mut postgres::Client,plan:PlayerPlan)->Result<(),String>{
 use postgres::types::ToSql;
 if plan.steam_ids.is_empty(){let mut tx=client.transaction().map_err(|e|format!("Player tags transaction failed: {e}"))?;return match tx.execute("DELETE FROM steam_account_tags WHERE steam_id IN () AND tag_id NOT IN (SELECT value::bigint FROM unnest($1::text[]) AS value)",&[&plan.tag_ids]){Err(e)=>Err(format!("Player tags delete failed: {e}")),Ok(_)=>Err("Unexpectedly accepted empty Steam ID query".into())}}
 let pairs=plan.steam_ids.len().checked_mul(plan.tag_ids.len()).ok_or("Player tag count overflow")?;
 // The original executes DELETE before its oversized INSERT fails; retain that
 // transactional ordering while avoiding an unrepresentable bind message.
 let mut tx=client.transaction().map_err(|e|format!("Player tags transaction failed: {e}"))?;
 tx.execute("DELETE FROM steam_account_tags WHERE steam_id = ANY($1::text[]) AND tag_id NOT IN (SELECT value::bigint FROM unnest($2::text[]) AS value)",&[&plan.steam_ids,&plan.tag_ids]).map_err(|e|format!("Player tags delete failed: {e}"))?;
 if pairs>32767{return Err("Player tag parameter limit exceeded after delete; transaction rollback required".into())}
 let mut sql=String::from("INSERT INTO steam_account_tags (steam_id, tag_id) VALUES ");let mut params:Vec<&(dyn ToSql+Sync)>=Vec::with_capacity(pairs*2);
 for steam in &plan.steam_ids {for id in &plan.tag_ids{if !params.is_empty(){sql.push(',')}let first=params.len()+1;sql.push_str(&format!("(${},${}::text::bigint)",first,first+1));params.push(steam);params.push(id)}}
 sql.push_str(" ON CONFLICT (steam_id, tag_id) DO NOTHING");
 tx.execute(&sql,&params).map_err(|e|format!("Player tags insert failed: {e}"))?;tx.commit().map_err(|e|format!("Player tags commit outcome may be unknown: {e}"))
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn checksum_plan_matches_original_unique_and_empty_tag_rules(){
  let p=serde_json::json!({"checksums":["a","b","a"],"tagIds":["2","1","2"]});assert_eq!(checksum_plan(&p),Ok(Some(ChecksumPlan{checksums:vec!["a".into(),"b".into(),"a".into()],tag_ids:vec!["2".into(),"1".into()]})));
  assert_eq!(checksum_plan(&serde_json::json!({"tagIds":[]})),Ok(None));
  assert!(checksum_plan(&serde_json::json!({"checksums":[],"tagIds":["1"]})).is_err());
  assert!(checksum_plan(&serde_json::json!({"checksums":["a"],"tagIds":[1]})).is_err());
 }

 #[test] fn delete_flow_commits_before_settings_and_push_before_success() {
  use std::cell::RefCell;let calls=RefCell::new(Vec::new());
  let result=delete_flow(||{calls.borrow_mut().push("delete");Ok(())},||{calls.borrow_mut().push("settings");Ok(Some(serde_json::json!({"x":1})))},|v|{assert_eq!(v["x"],1);calls.borrow_mut().push("push");Ok(())});
  assert_eq!(result,Ok(()));assert_eq!(*calls.borrow(),vec!["delete","settings","push"]);
 }
 #[test] fn delete_flow_stops_on_db_or_settings_failure_and_skips_unchanged_push() {
  use std::cell::RefCell;let calls=RefCell::new(Vec::new());
  assert!(delete_flow(||{calls.borrow_mut().push("delete");Err("db".into())},||{calls.borrow_mut().push("settings");Ok(None)},|_|{calls.borrow_mut().push("push");Ok(())}).is_err());assert_eq!(*calls.borrow(),vec!["delete"]);calls.borrow_mut().clear();
  assert!(delete_flow(||{calls.borrow_mut().push("delete");Ok(())},||{calls.borrow_mut().push("settings");Err("settings".into())},|_|{calls.borrow_mut().push("push");Ok(())}).is_err());assert_eq!(*calls.borrow(),vec!["delete","settings"]);calls.borrow_mut().clear();
  assert!(delete_flow(||{calls.borrow_mut().push("delete");Ok(())},||{calls.borrow_mut().push("settings");Ok(None)},|_|{calls.borrow_mut().push("push");Ok(())}).is_ok());assert_eq!(*calls.borrow(),vec!["delete","settings"]);
 }

 #[test] fn delete_cleans_exact_original_arrays_and_preserves_unrelated_values() {
  let mut value=serde_json::json!({"demos":{"tagIds":["3","9","3"],"keep":true},"matches":{"tagIds":["3",3]},"playerProfile":{"tagIds":["4","3"]},"teams":{"tagIds":["3"]}});
  assert_eq!(cleanup_tag_settings(&mut value,"3"),Ok(true));
  assert_eq!(value,serde_json::json!({"demos":{"tagIds":["9"],"keep":true},"matches":{"tagIds":[3]},"playerProfile":{"tagIds":["4"]},"teams":{"tagIds":["3"]}}));
 }
 #[test] fn delete_does_not_write_when_no_matching_tag_and_rejects_missing_parent() {
  let mut v=serde_json::json!({"demos":{"tagIds":null},"matches":{"tagIds":["4"]},"playerProfile":{"tagIds":[]}});
  let original=v.clone();assert_eq!(cleanup_tag_settings(&mut v,"3"),Ok(false));assert_eq!(v,original);
  let mut missing=serde_json::json!({"demos":{},"matches":{}});assert!(cleanup_tag_settings(&mut missing,"3").is_err());
 }

 #[test] fn source_validation_utf16_and_color() {
  assert_eq!(validate("", "#aBc123"), Err(3001));
  assert_eq!(validate(&"😀".repeat(21), "#abcdef"), Err(3002));
  assert_eq!(validate(&"😀".repeat(20), "#aBc123"), Ok(()));
  assert_eq!(validate("tag", "#12345g"), Err(3003));
  for suffix in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] { assert_eq!(validate("tag", &format!("#123456{suffix}")), Err(3003)); }
  assert_eq!(validate("tag", "#ﬀﬀﬀ"), Ok(()));
  assert_eq!(validate("tag", "#ﬀABCD"), Ok(()));
 }
 #[test] fn pg107_player_plan_original_unique_and_noop_contract(){
  let plan=player_plan(&serde_json::json!({"steamIds":["s1","s2","s1"],"tagIds":["2","1","2"]})).unwrap().unwrap();assert_eq!(plan.steam_ids,vec!["s1","s2","s1"]);assert_eq!(plan.tag_ids,vec!["2","1"]);
  assert!(player_plan(&serde_json::json!({"tagIds":[]})).unwrap().is_none());assert!(player_plan(&serde_json::json!({"steamIds":[],"tagIds":["1"]})).unwrap().unwrap().steam_ids.is_empty());assert!(player_plan(&serde_json::json!({"steamIds":[2],"tagIds":["1"]})).is_err());
 }

}
