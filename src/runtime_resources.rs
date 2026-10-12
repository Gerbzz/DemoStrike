//! Small first-party capture scripts travel in the binary. Extract to a
//! content-addressed per-user directory so an installed executable need not
//! have a writable installation folder or a source checkout.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const QC_SCRIPT: &[u8] = include_bytes!("../qc_event_pilot.ps1");

fn materialize_at(root: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let digest = format!("{:x}", Sha256::digest(bytes));
    let folder = root.join(&digest[..16]);
    std::fs::create_dir_all(&folder).map_err(|e| format!("cannot create capture resource folder: {e}"))?;
    let destination = folder.join(name);
    if destination.exists() {
        if std::fs::read(&destination).map_err(|e| e.to_string())? == bytes {
            return Ok(destination);
        }
        return Err(format!("embedded capture resource changed on disk: {}", destination.display()));
    }
    let mut temporary = tempfile::NamedTempFile::new_in(&folder)
        .map_err(|e| format!("cannot stage embedded capture script: {e}"))?;
    use std::io::Write;
    temporary.write_all(bytes).and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| format!("cannot sync embedded capture script: {e}"))?;
    match temporary.persist_noclobber(&destination) {
        Ok(_) => Ok(destination),
        Err(error) if destination.is_file()
            && std::fs::read(&destination).is_ok_and(|existing| existing == bytes) => {
                drop(error.file);
                Ok(destination)
            }
        Err(error) => Err(format!("cannot publish embedded capture script: {}", error.error)),
    }
}

pub fn qc_script_path() -> Result<PathBuf, String> {
    let data = std::env::var_os("LOCALAPPDATA")
        .ok_or("Windows LOCALAPPDATA is unavailable for embedded capture resources")?;
    materialize_at(&PathBuf::from(data).join("DemoStrike/resources"), "qc_event_pilot.ps1", QC_SCRIPT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocated_resource_root_uses_identical_embedded_bytes_and_refuses_tampering() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let a = materialize_at(first.path(), "qc_event_pilot.ps1", QC_SCRIPT).unwrap();
        let b = materialize_at(second.path(), "qc_event_pilot.ps1", QC_SCRIPT).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        std::fs::write(&b, b"tampered").unwrap();
        assert!(materialize_at(second.path(), "qc_event_pilot.ps1", QC_SCRIPT)
            .unwrap_err().contains("changed on disk"));
    }
}
