#![allow(non_snake_case, unexpected_cfgs, deprecated)]

// Bring in WebKit so WKWebView is available at runtime
#[link(name = "WebKit", kind = "framework")]
extern "C" {}

use crate::config::{self, StatusbarMode};
use crate::error::PulseError;
use crate::state::{self, AlertEntry, State};
use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSMenu, NSMenuItem,
    NSStatusBar, NSWindowStyleMask,
};
use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};

const POLL_SECS: f64 = 30.0;

struct MenubarInner {
    button: id,
    menu: id,
    panel: id,
    webview: id, // WKWebView
}

// ── helpers ──────────────────────────────────────────────────────────────────

unsafe fn ns_string(s: &str) -> id {
    NSString::alloc(nil).init_str(s)
}

unsafe fn set_sf_symbol(button: id, name: &str) {
    let has_sf: cocoa::base::BOOL = msg_send![
        class!(NSImage),
        respondsToSelector: sel!(imageWithSystemSymbolName:accessibilityDescription:)
    ];
    if has_sf != YES {
        return;
    }
    let image: id = msg_send![
        class!(NSImage),
        imageWithSystemSymbolName: ns_string(name)
        accessibilityDescription: nil
    ];
    if image != nil {
        let () = msg_send![image, setTemplate: YES];
        let () = msg_send![button, setImage: image];
    }
}

fn sev_sf_symbol(sev: Option<&str>) -> &'static str {
    match sev {
        Some("critical") => "exclamationmark.circle.fill",
        Some("warning") => "exclamationmark.triangle.fill",
        _ => "checkmark.circle.fill",
    }
}

/// Colored SF Symbol image for severity. Monochrome template fallback on macOS <12.
unsafe fn sev_image(sev: Option<&str>) -> id {
    let image: id = msg_send![
        class!(NSImage),
        imageWithSystemSymbolName: ns_string(sev_sf_symbol(sev))
        accessibilityDescription: nil
    ];
    if image == nil {
        return nil;
    }
    let has_color: cocoa::base::BOOL = msg_send![
        class!(NSImageSymbolConfiguration),
        respondsToSelector: sel!(configurationWithHierarchicalColor:)
    ];
    if has_color == YES {
        let color: id = match sev {
            Some("critical") => msg_send![class!(NSColor), systemRedColor],
            Some("warning") => msg_send![class!(NSColor), systemOrangeColor],
            _ => msg_send![class!(NSColor), systemGreenColor],
        };
        let cfg: id = msg_send![
            class!(NSImageSymbolConfiguration),
            configurationWithHierarchicalColor: color
        ];
        msg_send![image, imageWithSymbolConfiguration: cfg]
    } else {
        let () = msg_send![image, setTemplate: YES];
        image
    }
}

/// Attributed string for the detailed statusbar title.
unsafe fn detailed_attr_title(n_crit: usize, n_warn: usize, n_other: usize) -> id {
    let result: id = msg_send![class!(NSMutableAttributedString), new];
    let entries: &[(usize, Option<&str>)] = &[
        (n_crit, Some("critical")),
        (n_warn, Some("warning")),
        (n_other, None),
    ];
    let mut added = 0usize;
    for &(count, sev) in entries {
        if count == 0 {
            continue;
        }
        if added > 0 {
            let sep: id = msg_send![class!(NSAttributedString), alloc];
            let sep: id = msg_send![sep, initWithString: ns_string("  ")];
            let () = msg_send![result, appendAttributedString: sep];
        }
        added += 1;

        let img = sev_image(sev);
        if img != nil {
            let () = msg_send![img, setSize: NSSize { width: 13.0, height: 13.0 }];
            let attachment: id = msg_send![class!(NSTextAttachment), new];
            let () = msg_send![attachment, setImage: img];
            let img_str: id = msg_send![
                class!(NSAttributedString),
                attributedStringWithAttachment: attachment
            ];
            let () = msg_send![result, appendAttributedString: img_str];
        }

        let num: id = msg_send![class!(NSAttributedString), alloc];
        let num: id = msg_send![num, initWithString: ns_string(&count.to_string())];
        let () = msg_send![result, appendAttributedString: num];
    }
    result
}

fn sev_rank(sev: Option<&str>) -> u8 {
    match sev {
        Some("critical") => 0,
        Some("warning") => 1,
        _ => 2,
    }
}

fn format_duration(active_at: Option<&str>) -> String {
    let s = match active_at {
        Some(s) => s,
        None => return String::new(),
    };
    let dt = match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(dt) => dt,
        Err(_) => return String::new(),
    };
    let secs = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0) as u64;
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    if d > 0 {
        format!("{}d {}h", d, h)
    } else if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{}m", m.max(1))
    }
}

fn labels_tooltip(alert: &AlertEntry) -> String {
    let mut parts: Vec<_> = alert.labels.iter().collect();
    parts.sort_by_key(|(k, _)| k.as_str());
    parts
        .into_iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sort_alerts(alerts: &mut Vec<AlertEntry>) {
    alerts.sort_by(|a, b| {
        sev_rank(a.severity.as_deref())
            .cmp(&sev_rank(b.severity.as_deref()))
            .then_with(|| {
                a.active_at
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.active_at.as_deref().unwrap_or(""))
            })
    });
}

// ── alerts panel ─────────────────────────────────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn build_alerts_html(state: &State) -> String {
    let mut sources: Vec<_> = state.sources.iter().collect();
    sources.sort_by_key(|(n, _)| n.as_str());
    let total: usize = state.sources.values().map(|s| s.firing).sum();
    let all_alerts: Vec<_> = state.sources.values().flat_map(|s| s.alerts.iter()).collect();
    let n_crit = all_alerts.iter().filter(|a| a.severity.as_deref() == Some("critical")).count();
    let n_warn = all_alerts.iter().filter(|a| a.severity.as_deref() == Some("warning")).count();
    let n_other = total.saturating_sub(n_crit + n_warn);

    let mut body = String::new();

    // header
    body.push_str("<div class=\"header\"><h1>Alerts</h1><div class=\"badges\">");
    if n_crit > 0 { body.push_str(&format!("<span class=\"badge red\">{n_crit} critical</span>")); }
    if n_warn > 0 { body.push_str(&format!("<span class=\"badge orange\">{n_warn} warning</span>")); }
    if n_other > 0 { body.push_str(&format!("<span class=\"badge green\">{n_other} other</span>")); }
    if total == 0  { body.push_str("<span class=\"badge gray\">All clear</span>"); }
    body.push_str("</div></div>");

    // cards
    if total == 0 {
        body.push_str("<div class=\"empty\"><div class=\"empty-icon\">&#10003;</div><div class=\"empty-title\">No alerts firing</div></div>");
    } else {
        for (name, src) in &sources {
            if src.alerts.is_empty() && src.error.is_none() { continue; }
            body.push_str(&format!("<div class=\"source-label\">{}</div>", html_escape(name)));
            if let Some(ref err) = src.error {
                body.push_str(&format!("<div class=\"alert-card\"><span class=\"sev-pill critical\">error</span> {}</div>", html_escape(err)));
                continue;
            }
            let mut alerts = src.alerts.clone();
            sort_alerts(&mut alerts);
            for alert in &alerts {
                let sev = alert.severity.as_deref().unwrap_or("other");
                let cls = match sev { "critical" => "critical", "warning" => "warning", _ => "other" };
                let dur = format_duration(alert.active_at.as_deref());
                body.push_str("<div class=\"alert-card\">");
                body.push_str("<div class=\"alert-top\">");
                body.push_str(&format!("<span class=\"sev-pill {cls}\">{sev}</span>"));
                body.push_str(&format!("<span class=\"alert-name\">{}</span>", html_escape(&alert.name)));
                if !dur.is_empty() { body.push_str(&format!("<span class=\"duration\">{dur}</span>")); }
                body.push_str("</div>");
                let mut labels: Vec<_> = alert.labels.iter().filter(|(k,_)| k.as_str() != "alertname").collect();
                labels.sort_by_key(|(k,_)| k.as_str());
                if !labels.is_empty() {
                    body.push_str("<div class=\"labels\">");
                    for (k, v) in &labels {
                        body.push_str(&format!("<span class=\"chip\">{}={}</span>", html_escape(k), html_escape(v)));
                    }
                    body.push_str("</div>");
                }
                if let Some(s) = alert.annotations.get("summary") {
                    body.push_str(&format!("<div class=\"summary\">{}</div>", html_escape(s)));
                }
                body.push_str("</div>");
            }
        }
    }
    if let Some(ref ts) = state.fetched_at {
        body.push_str(&format!("<div class=\"footer\">Updated {}</div>", ts.format("%H:%M:%S UTC")));
    }

    // ponytail: CSS inlined — extract to file if it grows unwieldy
    format!(r###"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
:root{{
  color-scheme:light dark;
  --bg:#f5f5f7;--surface:#fff;--text:#1d1d1f;--text2:#6e6e73;
  --border:rgba(0,0,0,.07);--shadow:0 1px 4px rgba(0,0,0,.07),0 0 0 .5px rgba(0,0,0,.04);
  --red:#ff3b30;--red-bg:rgba(255,59,48,.1);
  --orange:#ff9500;--orange-bg:rgba(255,149,0,.1);
  --green:#34c759;--green-bg:rgba(52,199,89,.1);
  --chip:rgba(0,0,0,.05);--mono:'SF Mono',Menlo,monospace;
}}
@media(prefers-color-scheme:dark){{
  :root{{
    --bg:#1c1c1e;--surface:#2c2c2e;--text:#f2f2f7;--text2:#8e8e93;
    --border:rgba(255,255,255,.07);
    --shadow:0 1px 4px rgba(0,0,0,.4),0 0 0 .5px rgba(255,255,255,.04);
    --red-bg:rgba(255,59,48,.18);--orange-bg:rgba(255,149,0,.18);--green-bg:rgba(52,199,89,.18);
    --chip:rgba(255,255,255,.08);
  }}
}}
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:-apple-system,BlinkMacSystemFont,'Helvetica Neue',sans-serif;
  background:var(--bg);color:var(--text);font-size:13px;line-height:1.5;
  padding:20px;-webkit-font-smoothing:antialiased;}}
.header{{display:flex;justify-content:space-between;align-items:center;margin-bottom:22px;}}
h1{{font-size:20px;font-weight:700;letter-spacing:-.4px;}}
.badges{{display:flex;gap:6px;flex-wrap:wrap;justify-content:flex-end;}}
.badge{{font-size:11px;font-weight:600;padding:3px 10px;border-radius:20px;}}
.badge.red{{background:var(--red-bg);color:var(--red);}}
.badge.orange{{background:var(--orange-bg);color:var(--orange);}}
.badge.green{{background:var(--green-bg);color:var(--green);}}
.badge.gray{{background:var(--chip);color:var(--text2);}}
.source-label{{font-size:11px;font-weight:600;text-transform:uppercase;
  letter-spacing:.08em;color:var(--text2);margin:24px 0 8px;}}
.alert-card{{background:var(--surface);border:1px solid var(--border);
  border-radius:12px;padding:13px 15px;margin-bottom:8px;box-shadow:var(--shadow);}}
.alert-top{{display:flex;align-items:center;gap:9px;}}
.sev-pill{{font-size:10px;font-weight:700;text-transform:uppercase;
  letter-spacing:.06em;padding:2px 8px;border-radius:6px;white-space:nowrap;}}
.sev-pill.critical{{background:var(--red-bg);color:var(--red);}}
.sev-pill.warning{{background:var(--orange-bg);color:var(--orange);}}
.sev-pill.other{{background:var(--green-bg);color:var(--green);}}
.alert-name{{font-weight:600;flex:1;font-size:13px;}}
.duration{{font-size:11px;color:var(--text2);white-space:nowrap;}}
.labels{{display:flex;flex-wrap:wrap;gap:5px;margin-top:10px;}}
.chip{{font-size:10px;font-family:var(--mono);padding:2px 7px;
  border-radius:5px;background:var(--chip);color:var(--text2);}}
.summary{{font-size:12px;color:var(--text2);margin-top:8px;line-height:1.45;}}
.footer{{font-size:11px;color:var(--text2);text-align:center;
  margin-top:24px;padding-top:14px;border-top:1px solid var(--border);}}
.empty{{text-align:center;padding:60px 20px;color:var(--text2);}}
.empty-icon{{font-size:36px;margin-bottom:10px;color:var(--green);}}
.empty-title{{font-weight:600;font-size:16px;color:var(--text);}}
</style></head>
<body>{body}</body></html>"###)
}

// DEAD CODE below — replaced by WKWebView approach
#[allow(dead_code)]
unsafe fn create_alerts_panel() -> (id, id) {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(540.0, 500.0));

    let style = NSWindowStyleMask::NSTitledWindowMask
        | NSWindowStyleMask::NSClosableWindowMask
        | NSWindowStyleMask::NSResizableWindowMask
        | NSWindowStyleMask::NSMiniaturizableWindowMask;

    let panel: id = msg_send![class!(NSPanel), alloc];
    let panel: id = msg_send![
        panel,
        initWithContentRect: frame
        styleMask: style
        backing: NSBackingStoreType::NSBackingStoreBuffered
        defer: NO
    ];
    let () = msg_send![panel, setTitle: ns_string("Pulse — Alerts")];
    let () = msg_send![panel, center];
    let () = msg_send![panel, setReleasedWhenClosed: NO];
    let () = msg_send![panel, setFrameAutosaveName: ns_string("PulseAlertsPanel")];

    // WKWebView — has its own scroll view, use it directly as content view
    let wk_cfg: id = msg_send![class!(WKWebViewConfiguration), new];
    let webview: id = msg_send![class!(WKWebView), alloc];
    let webview: id = msg_send![webview, initWithFrame: frame configuration: wk_cfg];
    let () = msg_send![webview, setAutoresizingMask: 18usize]; // width|height sizable
    let () = msg_send![panel, setContentView: webview];

    (panel, webview)
}

unsafe fn refresh_panel_content(webview: id, state: &State) {
    let html = build_alerts_html(state);
    let () = msg_send![webview, loadHTMLString: ns_string(&html) baseURL: nil];
}

// ── tick ─────────────────────────────────────────────────────────────────────

extern "C" fn open_alerts_list(this: &Object, _: Sel, _: id) {
    unsafe {
        let inner_ptr = *this.get_ivar::<usize>("_inner") as *mut MenubarInner;
        let inner = &mut *inner_ptr;

        if inner.panel == nil {
            let (panel, wv) = create_alerts_panel();
            inner.panel = panel;
            inner.webview = wv;
        }

        let state = state::load();
        refresh_panel_content(inner.webview, &state);

        let () = msg_send![inner.panel, makeKeyAndOrderFront: nil];
        let app = NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory,
        );
        let () = msg_send![app, activateIgnoringOtherApps: YES];
    }
}

extern "C" fn tick(this: &Object, _: Sel, _: id) {
    unsafe {
        let inner_ptr = *this.get_ivar::<usize>("_inner") as *mut MenubarInner;
        let inner = &mut *inner_ptr;

        let state = state::load();
        let cfg = config::load().unwrap_or_default();

        let total: usize = state.sources.values().map(|s| s.firing).sum();

        let all_alerts: Vec<_> = state
            .sources
            .values()
            .filter(|s| s.error.is_none())
            .flat_map(|s| s.alerts.iter())
            .collect();
        let n_crit = all_alerts
            .iter()
            .filter(|a| a.severity.as_deref() == Some("critical"))
            .count();
        let n_warn = all_alerts
            .iter()
            .filter(|a| a.severity.as_deref() == Some("warning"))
            .count();
        let n_other = all_alerts
            .iter()
            .filter(|a| !matches!(a.severity.as_deref(), Some("critical") | Some("warning")))
            .count();

        // Button
        match cfg.statusbar_mode {
            StatusbarMode::Simple => {
                set_sf_symbol(inner.button, if total == 0 { "bell" } else { "bell.badge.fill" });
                let title = if total == 0 {
                    String::new()
                } else {
                    format!(" {total}")
                };
                let () = msg_send![inner.button, setTitle: ns_string(&title)];
            }
            StatusbarMode::Detailed => {
                let () = msg_send![inner.button, setImage: nil];
                if total == 0 {
                    let () = msg_send![inner.button, setTitle: ns_string("")];
                } else {
                    let attr = detailed_attr_title(n_crit, n_warn, n_other);
                    let () = msg_send![inner.button, setAttributedTitle: attr];
                }
            }
        }

        // ── Rebuild dropdown ──────────────────────────────────────────────────
        let item_count: usize = msg_send![inner.menu, numberOfItems];
        for _ in 0..item_count {
            let () = msg_send![inner.menu, removeItemAtIndex: 0usize];
        }

        // "Open Alerts List" at the top
        let open_item: id = msg_send![class!(NSMenuItem), new];
        let () = msg_send![open_item, setTitle: ns_string("Open Alerts List")];
        let () = msg_send![open_item, setAction: sel!(openAlertsList:)];
        let () = msg_send![open_item, setTarget: this as *const Object as id];
        let () = msg_send![inner.menu, addItem: open_item];

        let sep: id = NSMenuItem::separatorItem(nil);
        let () = msg_send![inner.menu, addItem: sep];

        if state.sources.is_empty() {
            let item: id = msg_send![class!(NSMenuItem), new];
            let () = msg_send![item, setTitle: ns_string("No sources configured")];
            let () = msg_send![item, setEnabled: NO];
            let () = msg_send![inner.menu, addItem: item];
        } else {
            let mut sources: Vec<_> = state.sources.iter().collect();
            sources.sort_by_key(|(name, _)| name.as_str());

            for (name, src) in sources {
                let header_title = if src.error.is_some() {
                    format!("{name}: error")
                } else {
                    format!("{name}: {} firing", src.firing)
                };
                let header: id = msg_send![class!(NSMenuItem), new];
                let () = msg_send![header, setTitle: ns_string(&header_title)];
                let () = msg_send![header, setEnabled: NO];
                let () = msg_send![inner.menu, addItem: header];

                if let Some(ref err) = src.error {
                    let short: String = err.chars().take(60).collect();
                    let err_item: id = msg_send![class!(NSMenuItem), new];
                    let () = msg_send![err_item, setTitle: ns_string(&format!("  {short}"))];
                    let () = msg_send![err_item, setEnabled: NO];
                    let () = msg_send![inner.menu, addItem: err_item];
                } else {
                    let mut alerts = src.alerts.clone();
                    sort_alerts(&mut alerts);

                    for alert in &alerts {
                        let dur = format_duration(alert.active_at.as_deref());
                        let label = if dur.is_empty() {
                            format!("  {}", alert.name)
                        } else {
                            format!("  {}  ({})", alert.name, dur)
                        };

                        let item: id = msg_send![class!(NSMenuItem), new];
                        let () = msg_send![item, setTitle: ns_string(&label)];

                        let img = sev_image(alert.severity.as_deref());
                        if img != nil {
                            let () = msg_send![item, setImage: img];
                        }

                        let () = msg_send![item, setEnabled: NO];

                        let tooltip = labels_tooltip(alert);
                        if !tooltip.is_empty() {
                            let () = msg_send![item, setToolTip: ns_string(&tooltip)];
                        }

                        let () = msg_send![inner.menu, addItem: item];
                    }
                }

                let sep: id = NSMenuItem::separatorItem(nil);
                let () = msg_send![inner.menu, addItem: sep];
            }
        }

        if let Some(ref ts) = state.fetched_at {
            let ts_item: id = msg_send![class!(NSMenuItem), new];
            let () = msg_send![
                ts_item,
                setTitle: ns_string(&format!("Updated {}", ts.format("%H:%M:%S")))
            ];
            let () = msg_send![ts_item, setEnabled: NO];
            let () = msg_send![inner.menu, addItem: ts_item];

            let sep: id = NSMenuItem::separatorItem(nil);
            let () = msg_send![inner.menu, addItem: sep];
        }

        let quit: id = msg_send![class!(NSMenuItem), new];
        let () = msg_send![quit, setTitle: ns_string("Quit pulse")];
        let () = msg_send![quit, setAction: sel!(terminate:)];
        let () = msg_send![quit, setTarget: NSApp()];
        let () = msg_send![inner.menu, addItem: quit];

        // Refresh panel if it's open
        if inner.panel != nil {
            let visible: cocoa::base::BOOL = msg_send![inner.panel, isVisible];
            if visible == YES {
                refresh_panel_content(inner.webview, &state);
            }
        }
    }
}

// ── run ───────────────────────────────────────────────────────────────────────

pub fn run() -> Result<(), PulseError> {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);

        let app = NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyProhibited,
        );

        let status_bar: id = NSStatusBar::systemStatusBar(nil);
        let status_item: id = status_bar.statusItemWithLength_(-1.0);
        let button: id = msg_send![status_item, button];

        let menu: id = NSMenu::new(nil);
        let () = msg_send![status_item, setMenu: menu];

        let mut decl = ClassDecl::new("PulseMenubarDelegate", class!(NSObject))
            .expect("PulseMenubarDelegate already registered");
        decl.add_ivar::<usize>("_inner");
        decl.add_method(sel!(tick:), tick as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(openAlertsList:),
            open_alerts_list as extern "C" fn(&Object, Sel, id),
        );
        let cls = decl.register();

        let inner_ptr = Box::into_raw(Box::new(MenubarInner {
            button,
            menu,
            panel: nil,
            webview: nil,
        }));

        let delegate: id = msg_send![cls, new];
        (*delegate).set_ivar("_inner", inner_ptr as usize);

        let _timer: id = msg_send![
            class!(NSTimer),
            scheduledTimerWithTimeInterval: POLL_SECS
            target: delegate
            selector: sel!(tick:)
            userInfo: nil
            repeats: YES
        ];

        tick(&*delegate, sel!(tick:), nil);

        app.run();
    }

    Ok(())
}
