use crate::error::PulseError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct State {
    pub fetched_at: Option<DateTime<Utc>>,
    pub sources: HashMap<String, SourceState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SourceState {
    pub firing: usize,
    pub alerts: Vec<AlertEntry>,
    pub error: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AlertEntry {
    pub name: String,
    pub severity: Option<String>,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub active_at: Option<String>,
}

pub fn state_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("pulse")
        .join("state.json")
}

pub fn load() -> State {
    let path = state_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(state: &State) -> Result<(), PulseError> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(state)?)?;
    std::fs::rename(tmp, &path)?;
    Ok(())
}
