use crate::{
    command::Evidence,
    connections::Mac,
    system::{PowerBackup, now},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub macs: Vec<Mac>,
    pub power_backup: Option<PowerBackup>,
    pub actions: Vec<Evidence>,
}
pub fn directory() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("Home directory unavailable")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Mac Link"))
}
pub fn load() -> Result<Preferences, String> {
    let path = directory()?.join("settings.json");
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Could not read saved Macs: {e}. Original file preserved.")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn save(prefs: &Preferences) -> Result<(), String> {
    let dir = directory()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(prefs).map_err(|e| e.to_string())?;
    let temp = dir.join(format!("settings.{}.{}.tmp", std::process::id(), now()));
    write_new(&temp, &bytes)?;
    fs::rename(temp, dir.join("settings.json")).map_err(|e| e.to_string())
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
pub fn export(text: &str) -> Result<PathBuf, String> {
    let dir = directory()?.join("Reports");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let path = dir.join(format!("Mac-mini-Remote-Access-{}.md", now()));
    write_new(&path, text.as_bytes())?;
    Ok(path)
}
