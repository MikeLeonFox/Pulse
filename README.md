# pulse

Statusbar widget that shows how many Prometheus / Mimir alerts are currently firing.

```
🔥 3        ← how many firing across all sources
```

Click (xbar) or run `pulse alerts` to see the full list.

---

## Install

```bash
brew tap MikeLeonFox/tap
brew install pulse
```

Or from source:

```bash
cargo install --git https://github.com/MikeLeonFox/Pulse
```

---

## Config

Create `~/.config/pulse/config.toml`:

```toml
interval_secs = 30          # how often the poller refreshes

[[source]]
name = "prod"
kind = "prometheus"          # or "mimir"
url  = "https://prometheus.example.com"
# bearer_token_env = "PULSE_TOKEN_PROD"   # reads from env var

[[source]]
name = "staging"
kind = "mimir"
url  = "https://mimir.example.com"
org_id = "my-tenant"                      # Mimir X-Scope-OrgID header
bearer_token_env = "PULSE_TOKEN_STAGING"
# insecure_skip_tls_verify = true         # for self-signed certs
```

Multiple sources of different kinds are fully supported. Each is fetched concurrently.

---

## Commands

| Command | What it does |
|---|---|
| `pulse poll` | Run the poller loop (keep in background / LaunchAgent) |
| `pulse poll --once` | Fetch once and exit |
| `pulse poll --interval 60` | Override interval |
| `pulse status` | Print `🔥 N` or `✅ 0` (reads state, no network) |
| `pulse status --format plain` | Print just the number |
| `pulse status --format xbar` | xbar / BitBar menu format |
| `pulse status --format json` | Full state as JSON |
| `pulse alerts` | List firing alerts grouped by source |
| `pulse alerts --source prod` | Filter to one source |
| `pulse alerts --json` | Raw JSON |
| `pulse info` | Show config + state summary |

---

## Integrations

### xbar / BitBar (macOS menubar)

Create `~/Library/Application Support/xbar/plugins/pulse.1m.sh`:

```bash
#!/usr/bin/env bash
exec /usr/local/bin/pulse status --format xbar
```

Make it executable: `chmod +x pulse.1m.sh`

The filename controls the refresh interval (`1m` = every minute).  
The poller daemon keeps state fresh on its own schedule — the xbar script just reads the cache.

### Run the poller as a LaunchAgent (macOS)

Create `~/Library/LaunchAgents/sh.pulse.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>         <string>sh.pulse</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/pulse</string>
    <string>poll</string>
  </array>
  <key>RunAtLoad</key>     <true/>
  <key>KeepAlive</key>     <true/>
  <key>StandardErrorPath</key>  <string>/tmp/pulse.log</string>
  <key>StandardOutPath</key>    <string>/tmp/pulse.log</string>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/sh.pulse.plist
```

### Starship prompt

```toml
# ~/.config/starship.toml
[custom.pulse]
command = "pulse status --format plain"
when    = "true"
format  = "🔥 [$output](red) "
```

### Waybar (Linux)

```json
"custom/pulse": {
    "exec": "pulse status --format plain",
    "interval": 30,
    "format": "🔥 {}",
    "on-click": "pulse alerts | fzf"
}
```

---

## Homebrew tap setup (one-time)

1. Create `github.com/MikeLeonFox/homebrew-tap` with a `Formula/` directory
2. Add a `TAP_TOKEN` secret to this repo (a GitHub PAT with `repo` scope on the tap)
3. The release workflow auto-updates the formula on every tagged release

---

## Development

```bash
cargo build
cargo test
cargo fmt
cargo clippy -- -D warnings
```

State lives at `~/.local/share/pulse/state.json`.  
Config lives at `~/.config/pulse/config.toml`.

Bump version in `Cargo.toml` + add entry to `CHANGELOG.md` to release.  
Never create tags manually — CI auto-tags on push to main.
