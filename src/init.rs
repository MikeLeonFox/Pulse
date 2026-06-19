use crate::config::{self, Config, Source, SourceKind};
use crate::error::PulseError;
use std::io::{self, Write};
use std::path::PathBuf;

const LABEL: &str = "sh.pulse.menubar";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

fn pulse_bin() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "pulse".to_string())
}

// ── stdin helpers ────────────────────────────────────────────────────────────

fn ask(prompt: &str) -> String {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    let _ = io::stdin().read_line(&mut buf);
    buf.trim().to_string()
}

fn ask_default(prompt: &str, default: &str) -> String {
    let input = ask(&format!("{prompt} [{default}]: "));
    if input.is_empty() {
        default.to_string()
    } else {
        input
    }
}

fn ask_optional(prompt: &str) -> Option<String> {
    let input = ask(&format!("{prompt} (leave blank to skip): "));
    if input.is_empty() {
        None
    } else {
        Some(input)
    }
}

// ── config wizard ────────────────────────────────────────────────────────────

fn build_config() -> Config {
    println!();
    println!("── Pulse config ───────────────────────────────");

    let interval_str = ask_default("Refresh interval (seconds)", "30");
    let interval_secs: u64 = interval_str.parse().unwrap_or(30);

    let mut sources: Vec<Source> = Vec::new();

    loop {
        println!();
        if sources.is_empty() {
            println!("Add your first source:");
        } else {
            let again = ask("Add another source? [y/N]: ");
            if !again.eq_ignore_ascii_case("y") {
                break;
            }
        }

        let name = ask_default("  Name", &format!("source{}", sources.len() + 1));

        let kind_str = ask_default("  Kind (prometheus / mimir)", "prometheus");
        let kind = if kind_str.to_lowercase().starts_with('m') {
            SourceKind::Mimir
        } else {
            SourceKind::Prometheus
        };

        let url = ask("  URL (e.g. https://prometheus.example.com): ");
        if url.is_empty() {
            println!("  ⚠ URL is required, skipping source.");
            continue;
        }

        let org_id = match kind {
            SourceKind::Mimir => ask_optional("  Mimir tenant / org_id"),
            SourceKind::Prometheus => None,
        };

        println!("  Auth — pick one (leave all blank for no auth):");
        let bearer_token_env =
            ask_optional("    Env var holding a static token (e.g. PULSE_TOKEN)");
        let token_command = if bearer_token_env.is_none() {
            ask_optional("    Command to fetch token (e.g. az account get-access-token --query accessToken -o tsv)")
        } else {
            None
        };

        let insecure_str = ask_default("  Skip TLS verify (for self-signed certs)?", "n");
        let insecure_skip_tls_verify = insecure_str.eq_ignore_ascii_case("y");

        sources.push(Source {
            name,
            kind,
            url,
            org_id,
            bearer_token: None,
            bearer_token_env,
            token_command,
            insecure_skip_tls_verify,
        });
    }

    Config {
        interval_secs,
        sources,
    }
}

fn write_config(cfg: &Config) -> Result<(), PulseError> {
    let path = config::config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut out = format!("interval_secs = {}\n", cfg.interval_secs);

    for src in &cfg.sources {
        out.push_str("\n[[sources]]\n");
        out.push_str(&format!("name = \"{}\"\n", src.name));
        out.push_str(&format!("kind = \"{}\"\n", src.kind));
        out.push_str(&format!("url  = \"{}\"\n", src.url));
        if let Some(ref id) = src.org_id {
            out.push_str(&format!("org_id = \"{id}\"\n"));
        }
        if let Some(ref env) = src.bearer_token_env {
            out.push_str(&format!("bearer_token_env = \"{env}\"\n"));
        }
        if let Some(ref cmd) = src.token_command {
            out.push_str(&format!("token_command = \"{cmd}\"\n"));
        }
        if src.insecure_skip_tls_verify {
            out.push_str("insecure_skip_tls_verify = true\n");
        }
    }

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, out.as_bytes())?;
    std::fs::rename(&tmp, &path)?;
    println!("✓ Wrote {}", path.display());
    Ok(())
}

// ── LaunchAgent ──────────────────────────────────────────────────────────────

fn install_launch_agent() -> Result<(), PulseError> {
    let bin = pulse_bin();
    let path = plist_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>menubar</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/pulse-menubar.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/pulse-menubar-error.log</string>
</dict>
</plist>
"#
    );

    let tmp = path.with_extension("plist.tmp");
    std::fs::write(&tmp, plist.as_bytes())?;
    std::fs::rename(&tmp, &path)?;
    println!("✓ Wrote {}", path.display());

    let _ = std::process::Command::new("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .output();

    let out = std::process::Command::new("launchctl")
        .args(["load", "-w", &path.to_string_lossy()])
        .output()?;

    if out.status.success() {
        println!("✓ Loaded — pulse menubar is running");
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        eprintln!("⚠ launchctl load failed: {stderr}");
    }

    Ok(())
}

// ── public entry points ──────────────────────────────────────────────────────

pub fn run() -> Result<(), PulseError> {
    let cfg_path = config::config_path();
    let existing = config::load().unwrap_or_default();

    if existing.sources.is_empty() {
        let cfg = build_config();
        if cfg.sources.is_empty() {
            eprintln!("No sources added — aborting.");
            return Ok(());
        }
        write_config(&cfg)?;

        println!();
        println!("── Testing connection ─────────────────────────");
        let out = std::process::Command::new(pulse_bin())
            .args(["poll", "--once"])
            .output();
        match out {
            Ok(o)
                if o.status.success()
                    && !String::from_utf8_lossy(&o.stderr).contains("no sources") =>
            {
                println!("✓ Fetched successfully");
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                eprintln!("⚠ {}", err.trim());
                eprintln!("  Check your URL and token, then run: pulse poll --once");
            }
            Err(e) => eprintln!("⚠ Could not run poll: {e}"),
        }
    } else {
        println!("✓ Config already exists at {}", cfg_path.display());
        println!("  {} source(s) configured", existing.sources.len());
    }

    println!();
    println!("── Installing menubar ─────────────────────────");
    install_launch_agent()?;

    println!();
    println!("Done. pulse is running in your menu bar.");
    println!("Run `pulse alerts` to see firing alerts.");

    Ok(())
}

pub fn uninstall() -> Result<(), PulseError> {
    println!("── Uninstalling pulse ─────────────────────────");

    // Stop and remove LaunchAgent
    let plist = plist_path();
    if plist.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist.to_string_lossy()])
            .output();
        std::fs::remove_file(&plist)?;
        println!("✓ Removed LaunchAgent");
    } else {
        println!("  LaunchAgent not installed — skipping");
    }

    // Config
    let cfg_path = config::config_path();
    if cfg_path.exists() {
        let answer = ask(&format!("Remove config at {}? [y/N]: ", cfg_path.display()));
        if answer.eq_ignore_ascii_case("y") {
            std::fs::remove_file(&cfg_path)?;
            println!("✓ Removed config");
        }
    }

    // State
    let state_path = crate::state::state_path();
    if state_path.exists() {
        let answer = ask(&format!(
            "Remove state at {}? [y/N]: ",
            state_path.display()
        ));
        if answer.eq_ignore_ascii_case("y") {
            std::fs::remove_file(&state_path)?;
            println!("✓ Removed state");
        }
    }

    println!();
    println!("Done. Remove the binary with: cargo uninstall pulse");

    Ok(())
}

pub fn stop() -> Result<(), PulseError> {
    let path = plist_path();
    if !path.exists() {
        println!("Not installed (no plist at {})", path.display());
        return Ok(());
    }

    let _ = std::process::Command::new("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .output();

    std::fs::remove_file(&path)?;
    println!("✓ Stopped and removed {}", path.display());

    Ok(())
}
