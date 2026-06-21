use crate::error::PulseError;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum StatusbarMode {
    #[default]
    Simple,
    Detailed,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub interval_secs: u64,
    pub sources: Vec<Source>,
    pub statusbar_mode: StatusbarMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval_secs: 30,
            sources: Vec::new(),
            statusbar_mode: StatusbarMode::Simple,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Source {
    pub name: String,
    pub kind: SourceKind,
    pub url: String,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub bearer_token: Option<String>,
    #[serde(default)]
    pub bearer_token_env: Option<String>,
    /// Shell command whose stdout is used as the bearer token (e.g. `az account get-access-token ...`)
    #[serde(default)]
    pub token_command: Option<String>,
    #[serde(default)]
    pub insecure_skip_tls_verify: bool,
}

impl Source {
    pub fn effective_token(&self) -> Option<String> {
        if let Some(ref t) = self.bearer_token {
            return Some(t.clone());
        }
        if let Some(ref env_var) = self.bearer_token_env {
            return std::env::var(env_var).ok();
        }
        if let Some(ref cmd) = self.token_command {
            let out = std::process::Command::new("sh")
                .args(["-c", cmd])
                .output()
                .ok()?;
            if out.status.success() {
                return Some(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
        }
        None
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Prometheus,
    Mimir,
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceKind::Prometheus => write!(f, "prometheus"),
            SourceKind::Mimir => write!(f, "mimir"),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config")
        .join("pulse")
        .join("config.toml")
}

pub fn load() -> Result<Config, PulseError> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let s = std::fs::read_to_string(&path)?;
    Ok(toml::from_str(&s)?)
}
