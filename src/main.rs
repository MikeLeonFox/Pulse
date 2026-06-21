use clap::{Parser, Subcommand, ValueEnum};
use std::process;

mod alerts;
mod client;
mod config;
mod error;
mod init;
#[cfg(target_os = "macos")]
mod menubar;
mod poller;
mod state;
mod statusline;

#[derive(Parser)]
#[command(
    name = "pulse",
    version,
    about = "Statusbar alert count from Prometheus / Mimir"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Poll all configured sources and write state to disk
    Poll {
        /// Override the configured interval (seconds)
        #[arg(short, long)]
        interval: Option<u64>,
        /// Poll once and exit instead of looping
        #[arg(long)]
        once: bool,
    },
    /// Print the current alert count (reads local state, no network)
    Status {
        #[arg(long, value_enum, default_value = "default")]
        format: OutputFormat,
    },
    /// List firing alerts
    Alerts {
        /// Filter to a single source by name
        #[arg(long)]
        source: Option<String>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Show resolved config and state summary
    Info,
    /// Run the native macOS menubar widget
    #[cfg(target_os = "macos")]
    Menubar,
    /// Install menubar LaunchAgent and walk through config
    #[cfg(target_os = "macos")]
    Setup,
    /// Remove pulse: LaunchAgent, config, and state
    #[cfg(target_os = "macos")]
    Uninstall,
    /// Restart the menubar process (unload + load LaunchAgent)
    #[cfg(target_os = "macos")]
    Reload,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    /// Icon + count (default for shell prompts / statusbars)
    Default,
    /// Raw count only — for scripting
    Plain,
    /// Full state as JSON
    Json,
    /// xbar / BitBar menu format
    Xbar,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result: Result<(), error::PulseError> = match cli.command {
        Commands::Poll { interval, once } => poller::run(interval, once).await,
        Commands::Status { format } => {
            let fmt = match format {
                OutputFormat::Default => statusline::Format::Default,
                OutputFormat::Plain => statusline::Format::Plain,
                OutputFormat::Json => statusline::Format::Json,
                OutputFormat::Xbar => statusline::Format::Xbar,
            };
            statusline::run(fmt);
            return;
        }
        Commands::Alerts { source, json } => {
            alerts::run(source, json);
            return;
        }
        Commands::Info => {
            show_info();
            return;
        }
        #[cfg(target_os = "macos")]
        Commands::Menubar => menubar::run(),
        #[cfg(target_os = "macos")]
        Commands::Setup => init::run(),
        #[cfg(target_os = "macos")]
        Commands::Uninstall => init::uninstall(),
        #[cfg(target_os = "macos")]
        Commands::Reload => {
            let plist = init::plist_path();
            if !plist.exists() {
                eprintln!("pulse: not installed (run: pulse setup)");
                process::exit(1);
            }
            let _ = std::process::Command::new("launchctl")
                .args(["unload", &plist.to_string_lossy()])
                .output();
            let out = std::process::Command::new("launchctl")
                .args(["load", "-w", &plist.to_string_lossy()])
                .output()
                .expect("launchctl failed");
            if out.status.success() {
                println!("✓ pulse reloaded");
            } else {
                eprintln!("pulse: {}", String::from_utf8_lossy(&out.stderr));
                process::exit(1);
            }
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("pulse: {e}");
        process::exit(1);
    }
}

fn show_info() {
    let cfg_path = config::config_path();
    println!("Config:  {}", cfg_path.display());

    match config::load() {
        Err(e) => println!("  🔴 {e}"),
        Ok(cfg) if cfg.sources.is_empty() => {
            println!("  🔔 no sources configured");
            println!();
            println!("Create {} with:", cfg_path.display());
            println!();
            println!("  interval_secs = 30");
            println!();
            println!("  [[source]]");
            println!("  name = \"prod\"");
            println!("  kind = \"prometheus\"   # or \"mimir\"");
            println!("  url  = \"https://prometheus.example.com\"");
            println!("  # bearer_token_env = \"PULSE_TOKEN\"");
            println!();
            println!("  [[source]]");
            println!("  name = \"staging\"");
            println!("  kind = \"mimir\"");
            println!("  url  = \"https://mimir.example.com\"");
            println!("  org_id = \"my-tenant\"");
        }
        Ok(cfg) => {
            println!("  Interval: {}s", cfg.interval_secs);
            for src in &cfg.sources {
                let token_hint = if src.effective_token().is_some() {
                    " [token ✓]"
                } else {
                    ""
                };
                println!("  [{} / {}] {}{}", src.name, src.kind, src.url, token_hint);
                if let Some(ref id) = src.org_id {
                    println!("    org_id: {id}");
                }
            }
        }
    }

    println!();
    let state_path = state::state_path();
    println!("State:   {}", state_path.display());

    let state = state::load();
    match state.fetched_at {
        None => println!("  never fetched — run: pulse poll --once"),
        Some(ref ts) => {
            println!("  last fetched: {}", ts.format("%Y-%m-%d %H:%M:%S UTC"));
            let total: usize = state.sources.values().map(|s| s.firing).sum();
            println!("  firing: {total}");
            for (name, src) in &state.sources {
                if let Some(ref err) = src.error {
                    println!("  [{name}] 🔴 {err}");
                } else {
                    println!("  [{name}] {} firing", src.firing);
                }
            }
        }
    }
}
