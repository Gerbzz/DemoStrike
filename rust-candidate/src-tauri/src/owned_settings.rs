use serde_json::{Map, Value};
use std::{fs::{self,File,OpenOptions},path::{Path,PathBuf},io::Write};

/// Task-owned fixture store. The caller must enforce the separate, immutable
/// process/profile policy before opening it. The held file lock coordinates
/// other cooperating instances; it is not a lock against arbitrary writers.
pub struct OwnedSettings {
    root:PathBuf,
    filename:&'static str,
    defaults:Value,
    recovering:Value,
    _lock:File,
    root_handle:same_file::Handle,
}

fn reject_reparse(path:&Path)->Result<(),String> {
    for component in path.ancestors() {
        match fs::symlink_metadata(component) {
            Ok(meta)=>{
                if meta.file_type().is_symlink(){return Err("Owned settings path contains a symlink".into());}
                #[cfg(windows)] {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {return Err("Owned settings path contains a reparse point".into());}
                }
            }
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
            Err(e)=>return Err(format!("Could not inspect owned settings path: {e}")),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn open_pinned_directory(path:&Path)->Result<same_file::Handle,String> {
    use std::os::windows::fs::{MetadataExt,OpenOptionsExt};
    // Open the directory itself, do not follow a final reparse point, and omit
    // FILE_SHARE_DELETE while this handle lives. Ancestors are not pinned.
    let file=OpenOptions::new().read(true).share_mode(0x1|0x2)
        .custom_flags(0x02000000|0x00200000).open(path)
        .map_err(|e|format!("Could not open owned profile directory: {e}"))?;
    let metadata=file.metadata().map_err(|e|format!("Could not inspect owned profile directory: {e}"))?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {return Err("Owned profile is not a plain directory".into());}
    same_file::Handle::from_file(file).map_err(|e|format!("Could not identify owned profile: {e}"))
}
#[cfg(not(windows))]
fn open_pinned_directory(_path:&Path)->Result<same_file::Handle,String> {Err("Owned profile trial is supported only on Windows".into())}

fn unsafe_inherited_key(key:&str,target:&Map<String,Value>)->bool {
    !target.contains_key(key) && matches!(key,"__proto__"|"constructor"|"toString"|"toLocaleString"|"valueOf"|"hasOwnProperty"|"isPrototypeOf"|"propertyIsEnumerable"|"__defineGetter__"|"__defineSetter__"|"__lookupGetter__"|"__lookupSetter__")
}

fn merge(target:&Value,source:&Value,replace_arrays:bool)->Value {
    match source {
        Value::Array(src)=>{
            let mut out=if !replace_arrays {target.as_array().cloned().unwrap_or_default()}else{Vec::new()};
            out.extend(src.iter().map(|v|clone_value(v,replace_arrays)));
            Value::Array(out)
        }
        Value::Object(src)=>{
            let target_map=target.as_object().cloned().unwrap_or_default();
            let mut out=Map::new();
            for (k,v) in &target_map {out.insert(k.clone(),clone_value(v,replace_arrays));}
            for (k,v) in src {
                if (k == "__proto__" && !target_map.contains_key(k)) || ((target.is_object() || target.is_array()) && unsafe_inherited_key(k,&target_map)){continue;}
                let merged=if (v.is_object()||v.is_array()) && target_map.contains_key(k) {merge(&target_map[k],v,replace_arrays)} else {clone_value(v,replace_arrays)};
                out.insert(k.clone(),merged);
            }
            Value::Object(out)
        }
        _=>source.clone(),
    }
}
fn clone_value(value:&Value,replace_arrays:bool)->Value {
    if value.is_object(){merge(&Value::Object(Map::new()),value,replace_arrays)}
    else if value.is_array(){merge(&Value::Array(Vec::new()),value,replace_arrays)}
    else{value.clone()}
}

impl OwnedSettings {
    fn open(root:PathBuf,defaults:Value)->Result<Self,String> {Self::open_named(root,defaults,"csdm-settings.json")}
    fn open_named(root:PathBuf,defaults:Value,filename:&'static str)->Result<Self,String> {
        if !defaults.is_object(){return Err("Owned settings defaults must be an object".into());}
        reject_reparse(&root)?;
        reject_reparse(&root)?;
        let root_handle=open_pinned_directory(&root)?;
        let lock_path=root.join(".owned-settings.lock");reject_reparse(&lock_path)?;
        let lock=OpenOptions::new().read(true).write(true).create(true).open(&lock_path).map_err(|e|format!("Could not open owned settings lock: {e}"))?;
        lock.try_lock().map_err(|e|format!("Owned settings profile is already in use: {e}"))?;
        Ok(Self{root,filename,defaults:defaults.clone(),recovering:defaults,_lock:lock,root_handle})
    }
    fn settings_path(&self)->Result<PathBuf,String> {
        reject_reparse(&self.root)?;
        if open_pinned_directory(&self.root)? != self.root_handle {return Err("Owned profile directory identity changed".into());}
        let path=self.root.join(self.filename);reject_reparse(&path)?;Ok(path)
    }
    fn write_value(&self,value:&Value)->Result<(),String> {
        let path=self.settings_path()?;let bytes=serde_json::to_vec_pretty(value).map_err(|e|format!("Could not serialize owned settings: {e}"))?;
        let mut pending=tempfile::NamedTempFile::new_in(&self.root).map_err(|e|format!("Could not create owned settings temporary file: {e}"))?;
        pending.write_all(&bytes).map_err(|e|format!("Could not write owned settings: {e}"))?;
        pending.as_file().sync_all().map_err(|e|format!("Could not sync owned settings: {e}"))?;
        reject_reparse(&path)?;
        pending.persist(path).map_err(|e|format!("Could not replace owned settings: {}",e.error))?;
        Ok(())
    }
    pub fn load(&mut self)->Result<Value,String> {
        let path=self.settings_path()?;
        match fs::read(path) {
            Ok(bytes)=>match serde_json::from_slice::<Value>(&bytes){Ok(value)=>{self.recovering=value.clone();Ok(value)},Err(_)=>Ok(self.recovering.clone())},
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{let defaults=self.defaults.clone();if self.write_value(&defaults).is_ok(){Ok(defaults)}else{Ok(self.recovering.clone())}},
            Err(_)=>Ok(self.recovering.clone()),
        }
    }
    pub fn update(&mut self,partial:&Value,replace_arrays:bool)->Result<Value,String> {
        if !partial.is_object(){return Err("Owned settings update must be an object".into());}
        let current=self.load()?;let updated=merge(&current,partial,replace_arrays);self.write_value(&updated)?;Ok(updated)
    }
}

/// Serializes owned profile loads and partial updates in this process. Whole
/// snapshots retain the existing save contract and may overwrite newer values.
/// The underlying file lock coordinates only cooperating processes.
pub struct OwnedSettingsCoordinator<S> { inner: std::sync::Mutex<S> }
pub trait CoordinatedSettings {
    fn load_settings(&mut self) -> Result<Value,String>;
    fn update_settings(&mut self, partial:&Value, replace_arrays:bool) -> Result<Value,String>;
    fn save_settings(&mut self, settings:&Value) -> Result<(),String>;
}
impl CoordinatedSettings for OwnedSettings {
    fn load_settings(&mut self)->Result<Value,String> { self.load() }
    fn update_settings(&mut self,p:&Value,r:bool)->Result<Value,String> { self.update(p,r) }
    fn save_settings(&mut self,v:&Value)->Result<(),String> { self.write_value(v) }
}
impl<S:CoordinatedSettings> OwnedSettingsCoordinator<S> {
    fn from_store(store:S)->Self { Self {inner:std::sync::Mutex::new(store)} }
    pub fn load(&self)->Result<Value,String> {self.inner.lock().map_err(|_|"Owned settings lock is poisoned".to_string())?.load_settings()}
    pub fn update(&self,p:&Value,r:bool)->Result<Value,String> {self.inner.lock().map_err(|_|"Owned settings lock is poisoned".to_string())?.update_settings(p,r)}
    pub fn save_whole(&self,snapshot:&Value)->Result<(),String> {self.inner.lock().map_err(|_|"Owned settings lock is poisoned".to_string())?.save_settings(snapshot)}
}
fn validate_trial_scope(flag:Option<&std::ffi::OsStr>, root:&Path)->Result<(),String> {
    const OWNED_ROOT:&str=r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg8-native-profile";
    const PG18_ROOT:&str=r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg18-green-profile";
    let valid=(flag==Some(std::ffi::OsStr::new("pg80")) && root==Path::new(r"C:\REDACTED\Documents\Codex\2026-10-29\task-2\pg80-fixture\profile")) || (flag==Some(std::ffi::OsStr::new("pg79")) && root==Path::new(r"C:\REDACTED\Documents\Codex\2026-10-29\task-2\pg79-fixture\profile")) || (flag==Some(std::ffi::OsStr::new("1")) && root==Path::new(OWNED_ROOT)) || (flag==Some(std::ffi::OsStr::new("pg18")) && root==Path::new(PG18_ROOT));
    #[cfg(test)] let valid=valid || (flag==Some(std::ffi::OsStr::new("pg18")) && match std::env::var("PG20_CASE").as_deref() {Ok("missing")=>root==Path::new(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg20-missing-profile"),Ok("error")=>root==Path::new(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg20-error-profile"),Ok("recovery")=>root==Path::new(r"C:\REDACTED\Documents\Codex\2026-10-27\task-6\pg21-green-profile"),_=>false});
    if !valid {return Err("Owned settings target is outside the fixed trial profile".into());}
    Ok(())
}
fn create_fresh_profile(root:&Path)->Result<(),String> {
    reject_reparse(root)?;
    fs::create_dir(root).map_err(|e|format!("Could not create a fresh owned settings profile: {e}"))
}
impl OwnedSettingsCoordinator<OwnedSettings> {
    /// Rust-side trusted bootstrap supplies an existing profile; renderer data never selects it.
    pub(super) fn open_product(root:&Path,defaults:Value)->Result<Self,String>{if !root.is_absolute(){return Err("Trusted profile path must be absolute".into())}Ok(Self::from_store(OwnedSettings::open_named(root.to_path_buf(),defaults,"settings.json")?))}
    // initializeSettings mutates the object retained by getSettings before
    // awaiting writeSettings, including if the write later fails.
    pub fn save_initialized(&self,snapshot:&Value)->Result<(),String> {
        let mut store=self.inner.lock().map_err(|_|"Owned settings lock is poisoned".to_string())?;
        store.recovering=snapshot.clone();
        store.write_value(snapshot)
    }
}
impl OwnedSettingsCoordinator<OwnedSettings> {
    /// The trial authority is fixed in code, never read from mutable settings.
    pub fn open_owned(root:&Path,defaults:Value)->Result<Self,String> {
        validate_trial_scope(std::env::var_os("DEMOSTRIKE_OWNED_PG_TRIAL").as_deref(), root)?;
        reject_reparse(root)?;
        match fs::create_dir(root) { Ok(())=>{}, Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{}, Err(e)=>return Err(format!("Could not create owned settings profile: {e}")) }
        Ok(Self::from_store(OwnedSettings::open(root.to_path_buf(),defaults)?))
    }
}


