//! Session file: known repos, open tabs and the last active tab.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct Data {
    pub known: Vec<String>,
    pub open: Vec<String>,
    pub active: usize,
}

fn file() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("grit");
    if !path.is_dir() {
        fs::create_dir_all(&path).ok()?;
    }
    path.push("session.json");
    Some(path)
}

pub(super) fn load() -> Data {
    file()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(super) fn save(data: &Data) {
    let Some(path) = file() else { return };
    if let Ok(content) = serde_json::to_string_pretty(data) {
        fs::write(path, content).ok();
    }
}
