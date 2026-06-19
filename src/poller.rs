use crate::client;
use crate::config::{self, Config};
use crate::error::PulseError;
use crate::state::{self, SourceState};
use chrono::Utc;
use std::time::Duration;
use tokio::time;

pub async fn run(interval_override: Option<u64>, once: bool) -> Result<(), PulseError> {
    let cfg = config::load()?;

    if cfg.sources.is_empty() {
        eprintln!(
            "pulse: no sources configured.\nCreate {}",
            config::config_path().display()
        );
        return Ok(());
    }

    let interval = Duration::from_secs(interval_override.unwrap_or(cfg.interval_secs));

    loop {
        poll_once(&cfg).await;
        if once {
            break;
        }

        tokio::select! {
            _ = time::sleep(interval) => {}
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    Ok(())
}

async fn poll_once(cfg: &Config) {
    let mut state = state::load();
    state.fetched_at = Some(Utc::now());

    let handles: Vec<_> = cfg
        .sources
        .iter()
        .map(|source| {
            let source = source.clone();
            tokio::spawn(async move {
                let client = match client::build_client(source.insecure_skip_tls_verify) {
                    Ok(c) => c,
                    Err(e) => return (source.name.clone(), Err(e)),
                };
                let result = client::fetch_alerts(&client, &source).await;
                (source.name.clone(), result)
            })
        })
        .collect();

    for handle in handles {
        let Ok((name, result)) = handle.await else {
            continue;
        };

        let source_state = match result {
            Ok(alerts) => SourceState {
                firing: alerts.len(),
                alerts,
                error: None,
                fetched_at: Some(Utc::now()),
            },
            Err(e) => {
                // preserve last known good alerts, just update the error
                let existing = state.sources.get(&name).cloned().unwrap_or_default();
                SourceState {
                    error: Some(e.to_string()),
                    fetched_at: Some(Utc::now()),
                    ..existing
                }
            }
        };

        state.sources.insert(name, source_state);
    }

    if let Err(e) = state::save(&state) {
        eprintln!("pulse: failed to save state: {e}");
    }
}
