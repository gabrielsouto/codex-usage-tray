use std::sync::mpsc;
use std::time::Duration;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::config::Config;
use crate::i18n::tr;
use crate::state::AppState;
use crate::{icon, notify, usage};

enum UserEvent {
    Menu(MenuEvent),
    Usage(Result<usage::Snapshot, String>),
}

pub fn run() {
    let cfg = Config::load();
    let lang = cfg.lang();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    let menu = Menu::new();
    let mi_refresh = MenuItem::new(tr(lang, "refresh"), true, None);
    let mi_page = MenuItem::new(tr(lang, "open_usage"), true, None);
    let mi_cfg = MenuItem::new(tr(lang, "open_config"), true, None);
    let mi_quit = MenuItem::new(tr(lang, "quit"), true, None);
    menu.append_items(&[
        &mi_refresh,
        &mi_page,
        &mi_cfg,
        &PredefinedMenuItem::separator(),
        &mi_quit,
    ])
    .expect("failed to build tray menu");

    let (wake_tx, wake_rx) = mpsc::channel::<()>();
    {
        let cfg = cfg.clone();
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || loop {
            let res = usage::fetch(&cfg);
            if proxy.send_event(UserEvent::Usage(res)).is_err() {
                break;
            }
            let _ = wake_rx.recv_timeout(Duration::from_secs(cfg.poll_interval_secs.max(60)));
        });
    }

    let mut tray: Option<TrayIcon> = None;
    let mut app = AppState::new(cfg.clone());

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                tray = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_tooltip(tr(lang, "loading"))
                        .with_icon(icon::make(None, &cfg))
                        .build()
                        .expect("failed to create tray icon"),
                );
            }
            Event::UserEvent(UserEvent::Menu(e)) => {
                if e.id == mi_quit.id() {
                    tray.take();
                    *control_flow = ControlFlow::Exit;
                } else if e.id == mi_refresh.id() {
                    let _ = wake_tx.send(());
                } else if e.id == mi_page.id() {
                    let _ = open::that("https://chatgpt.com/codex/settings/usage");
                } else if e.id == mi_cfg.id() {
                    let _ = open::that(Config::path());
                }
            }
            Event::UserEvent(UserEvent::Usage(res)) => {
                let out = app.apply(res);
                if let Some(t) = &tray {
                    let _ = t.set_tooltip(Some(&out.tooltip));
                    let _ = t.set_icon(Some(icon::make(out.utilization, &cfg)));
                }
                for (title, body) in out.notifications {
                    notify::toast(&title, &body);
                }
            }
            _ => {}
        }
    });
}
