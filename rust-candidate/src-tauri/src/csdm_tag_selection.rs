//! Tag-dialog selection behavior over valid Unicode tag IDs.
//! DOM ownership and the renderer bridge remain separate integration work.
/// Matches the original asymmetric length-and-membership comparison,
/// including its treatment of duplicate values.
pub fn same_values(a:&[String],b:&[String])->bool{a.len()==b.len()&&a.iter().all(|value|b.contains(value))}
pub struct TagSelection{current_default:Vec<String>,selected:Vec<String>}
impl TagSelection{
 pub fn new(initial:Vec<String>)->Self{Self{selected:initial.clone(),current_default:initial}}
 pub fn selected(&self)->&[String]{&self.selected}
 /// React preserves the selected state when the caller changes its default prop.
 pub fn set_default(&mut self,current:Vec<String>){self.current_default=current}
 pub fn is_selected(&self,id:&str)->bool{self.selected.iter().any(|value|value==id)}
 pub fn toggle(&mut self,id:&str){if self.is_selected(id){self.selected.retain(|value|value!=id)}else{self.selected.push(id.to_owned())}}
 pub fn changed(&self)->bool{!same_values(&self.selected,&self.current_default)}
}
#[cfg(test)] mod tests{
 use super::*;use serde_json::Value;
 fn strings(v:&Value)->Vec<String>{v.as_array().unwrap().iter().map(|v|v.as_str().unwrap().to_owned()).collect()}
 #[test]fn pg100_original_dialog_string_selection_oracle(){
  let oracle:Value=serde_json::from_str(include_str!("../tests/fixtures/pg100-tag-selection-oracle-v2.json")).unwrap();
  let comparisons=oracle["comparisons"].as_array().unwrap();assert_eq!(comparisons.len(),7225);
  for row in comparisons{let a=strings(&row["a"]);let b=strings(&row["b"]);assert_eq!(same_values(&a,&b),row["same"].as_bool().unwrap(),"{a:?} versus {b:?}");}
  let toggles=oracle["toggles"].as_array().unwrap();assert_eq!(toggles.len(),340);
  for row in toggles{let input=strings(&row["current"]);let id=row["id"].as_str().unwrap();let mut state=TagSelection::new(input.clone());assert_eq!(state.is_selected(id),row["selected"].as_bool().unwrap(),"{input:?} / {id:?}");state.toggle(id);assert_eq!(state.selected(),strings(&row["next"]));assert_eq!(state.changed(),row["changed"].as_bool().unwrap());}
  let traces=oracle["traces"].as_array().unwrap();assert_eq!(traces.len(),850);
  for row in traces{let mut state=TagSelection::new(strings(&row["initial"]));state.set_default(strings(&row["currentDefault"]));assert_eq!(state.changed(),row["changedBefore"].as_bool().unwrap());for step in row["steps"].as_array().unwrap(){state.toggle(step["id"].as_str().unwrap());assert_eq!(state.selected(),strings(&step["selected"]));assert_eq!(state.changed(),step["changed"].as_bool().unwrap());}}
 }
}
