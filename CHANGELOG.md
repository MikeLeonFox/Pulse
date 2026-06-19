# Changelog

## [0.1.0] - unreleased

### Added
- `pulse poll` — polls Prometheus / Mimir sources, writes state atomically
- `pulse status` — reads local state, outputs alert count (no network, fast)
- `pulse alerts` — lists firing alerts grouped by source, sorted by severity
- `pulse info` — shows resolved config and state summary
- Multiple source support (mix Prometheus and Mimir freely)
- Per-source: bearer token (inline or env var), `X-Scope-OrgID`, TLS skip
- Output formats: default (icon+count), plain, JSON, xbar
- xbar / BitBar menubar format with per-alert drilldown
- Atomic state writes (`state.json.tmp` → rename)
- GitHub Actions: CI (fmt, clippy, test), auto-tag, cross-platform release
- Homebrew tap auto-update on release
