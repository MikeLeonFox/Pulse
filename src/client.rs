use crate::config::{Source, SourceKind};
use crate::error::PulseError;
use crate::state::AlertEntry;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Deserialize)]
struct PrometheusResponse {
    data: PrometheusData,
}

#[derive(Deserialize)]
struct PrometheusData {
    alerts: Vec<RawAlert>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAlert {
    labels: HashMap<String, String>,
    #[serde(default)]
    annotations: HashMap<String, String>,
    state: String,
    active_at: Option<String>,
}

pub fn build_client(insecure: bool) -> Result<Client, PulseError> {
    let builder = Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5));
    let builder = if insecure {
        builder.danger_accept_invalid_certs(true)
    } else {
        builder
    };
    Ok(builder.build()?)
}

pub async fn fetch_alerts(client: &Client, source: &Source) -> Result<Vec<AlertEntry>, PulseError> {
    let url = match source.kind {
        SourceKind::Prometheus => {
            format!("{}/api/v1/alerts", source.url.trim_end_matches('/'))
        }
        SourceKind::Mimir => {
            format!(
                "{}/prometheus/api/v1/alerts",
                source.url.trim_end_matches('/')
            )
        }
    };

    let mut req = client.get(&url);

    if let Some(token) = source.effective_token() {
        req = req.bearer_auth(token);
    }

    if let Some(ref org_id) = source.org_id {
        req = req.header("X-Scope-OrgID", org_id);
    }

    let resp: PrometheusResponse = req.send().await?.error_for_status()?.json().await?;

    let alerts = resp
        .data
        .alerts
        .into_iter()
        .filter(|a| a.state == "firing")
        .map(|a| AlertEntry {
            name: a.labels.get("alertname").cloned().unwrap_or_default(),
            severity: a.labels.get("severity").cloned(),
            active_at: a.active_at,
            labels: a.labels,
            annotations: a.annotations,
        })
        .collect();

    Ok(alerts)
}
