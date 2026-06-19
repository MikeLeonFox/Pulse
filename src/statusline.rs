use crate::state::{self, State};

pub enum Format {
    Default,
    Plain,
    Json,
    Xbar,
}

pub fn run(format: Format) {
    let state = state::load();
    let total: usize = state.sources.values().map(|s| s.firing).sum();

    match format {
        Format::Plain => println!("{total}"),
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&state).unwrap_or_default()
        ),
        Format::Default => {
            if total == 0 {
                println!("0 firing");
            } else {
                println!("{total} firing");
            }
        }
        Format::Xbar => print_xbar(&state, total),
    }
}

fn print_xbar(state: &State, total: usize) {
    if total == 0 {
        println!("0 | color=green");
    } else {
        println!("{total} | color=red");
    }
    println!("---");

    if state.sources.is_empty() {
        println!("No sources configured | color=gray");
        println!("Run: pulse info | bash='pulse info' terminal=true");
        return;
    }

    for (name, src) in &state.sources {
        if let Some(ref err) = src.error {
            println!("{name}: ⚠ error | color=orange");
            println!("--{err} | color=gray");
        } else {
            let color = if src.firing > 0 { "red" } else { "green" };
            println!("{name}: {} firing | color={color}", src.firing);
            for alert in &src.alerts {
                let sev_color = match alert.severity.as_deref() {
                    Some("critical") => "red",
                    Some("warning") => "orange",
                    _ => "black",
                };
                println!("--{} | color={sev_color}", alert.name);
            }
        }
    }

    println!("---");
    if let Some(ref ts) = state.fetched_at {
        println!("Last updated: {} | color=gray", ts.format("%H:%M:%S"));
    }
    println!("Refresh | refresh=true");
    println!("List alerts | bash='pulse alerts' terminal=true");
}
