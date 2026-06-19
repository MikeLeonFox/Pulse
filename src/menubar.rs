#![allow(non_snake_case, unexpected_cfgs, deprecated)]

use crate::error::PulseError;
use crate::state;
use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicy, NSMenu, NSMenuItem, NSStatusBar,
};
use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSAutoreleasePool, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};

const POLL_SECS: f64 = 30.0;

struct MenubarInner {
    button: id,
    menu: id,
}

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
    let sym_ns = ns_string(name);
    let image: id = msg_send![
        class!(NSImage),
        imageWithSystemSymbolName: sym_ns
        accessibilityDescription: nil
    ];
    if image != nil {
        let () = msg_send![image, setTemplate: YES];
        let () = msg_send![button, setImage: image];
    }
}

extern "C" fn tick(this: &Object, _: Sel, _: id) {
    unsafe {
        let inner_ptr = *this.get_ivar::<usize>("_inner") as *mut MenubarInner;
        let inner = &*inner_ptr;

        let state = state::load();
        let total: usize = state.sources.values().map(|s| s.firing).sum();

        // Icon: bell when clear, bell.badge.fill when firing
        let sym = if total == 0 {
            "bell"
        } else {
            "bell.badge.fill"
        };
        set_sf_symbol(inner.button, sym);

        // Title: empty when clear, count when firing
        let title = if total == 0 {
            String::new()
        } else {
            format!(" {total}")
        };
        let () = msg_send![inner.button, setTitle: ns_string(&title)];

        // Rebuild dropdown menu
        let item_count: usize = msg_send![inner.menu, numberOfItems];
        for _ in 0..item_count {
            let () = msg_send![inner.menu, removeItemAtIndex: 0usize];
        }

        // Per-source items
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
                    let short = if err.len() > 60 {
                        &err[..60]
                    } else {
                        err.as_str()
                    };
                    let err_item: id = msg_send![class!(NSMenuItem), new];
                    let () = msg_send![err_item, setTitle: ns_string(&format!("  {short}"))];
                    let () = msg_send![err_item, setEnabled: NO];
                    let () = msg_send![inner.menu, addItem: err_item];
                } else {
                    let mut alerts = src.alerts.clone();
                    alerts.sort_by(|a, b| {
                        let rank = |s: &Option<String>| match s.as_deref() {
                            Some("critical") => 0,
                            Some("warning") => 1,
                            _ => 2,
                        };
                        rank(&a.severity).cmp(&rank(&b.severity))
                    });

                    for alert in &alerts {
                        let sev = alert.severity.as_deref().unwrap_or("unknown");
                        let label = format!("  {} [{}]", alert.name, sev);
                        let item: id = msg_send![class!(NSMenuItem), new];
                        let () = msg_send![item, setTitle: ns_string(&label)];
                        let () = msg_send![item, setEnabled: NO];
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
    }
}

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
        let cls = decl.register();

        let inner_ptr = Box::into_raw(Box::new(MenubarInner { button, menu }));

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
