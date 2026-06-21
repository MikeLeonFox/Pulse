use crate::state;

pub fn run(source_filter: Option<String>, json: bool) {
    let state = state::load();

    if json {
        let filtered: std::collections::HashMap<_, _> = state
            .sources
            .iter()
            .filter(|(name, _)| source_filter.as_deref().is_none_or(|f| name.as_str() == f))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&filtered).unwrap_or_default()
        );
        return;
    }

    let total: usize = state
        .sources
        .iter()
        .filter(|(name, _)| source_filter.as_deref().is_none_or(|f| name.as_str() == f))
        .map(|(_, s)| s.firing)
        .sum();

    if total == 0 {
        println!("🔔 No alerts firing");
        return;
    }

    let mut sources: Vec<_> = state.sources.iter().collect();
    sources.sort_by_key(|(name, _)| name.as_str());

    for (name, src) in sources {
        if let Some(ref f) = source_filter {
            if name != f {
                continue;
            }
        }
        if src.alerts.is_empty() {
            continue;
        }

        println!("\n🔔  {} — {} firing", name, src.firing);
        println!("{}", "─".repeat(48));

        let mut alerts = src.alerts.clone();
        alerts.sort_by(|a, b| {
            let sev_rank = |s: &Option<String>| match s.as_deref() {
                Some("critical") => 0,
                Some("warning") => 1,
                _ => 2,
            };
            sev_rank(&a.severity).cmp(&sev_rank(&b.severity))
        });

        for alert in &alerts {
            let sev = alert.severity.as_deref().unwrap_or("unknown");
            let icon = match sev {
                "critical" => "🔴",
                "warning" => "🟠",
                _ => "🟢",
            };
            println!("{icon}  {} [{}]", alert.name, sev);

            if let Some(ref ts) = alert.active_at {
                println!("    since: {ts}");
            }
            if let Some(summary) = alert.annotations.get("summary") {
                println!("    {summary}");
            }

            let skip = ["alertname", "severity"];
            let mut extra: Vec<_> = alert
                .labels
                .iter()
                .filter(|(k, _)| !skip.contains(&k.as_str()))
                .collect();
            extra.sort_by_key(|(k, _)| k.as_str());
            for (k, v) in extra {
                println!("    {k}={v}");
            }
        }
    }

    println!();
}
