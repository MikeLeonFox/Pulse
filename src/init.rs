use crate::error::PulseError;
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

pub fn run() -> Result<(), PulseError> {
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

    // Unload first in case it was already loaded (idempotent)
    let _ = std::process::Command::new("launchctl")
        .args(["unload", &path.to_string_lossy()])
        .output();

    let out = std::process::Command::new("launchctl")
        .args(["load", "-w", &path.to_string_lossy()])
        .output()?;

    if out.status.success() {
        println!("✓ Loaded — pulse menubar is running");
        println!();
        println!("To stop:    launchctl unload {}", path.display());
        println!("To restart: launchctl kickstart gui/$(id -u)/{LABEL}");
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        eprintln!("⚠ launchctl load failed: {stderr}");
    }

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
