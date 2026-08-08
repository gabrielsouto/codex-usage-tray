#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod i18n;
mod icon;
mod notify;
mod state;
mod usage;

#[cfg(windows)]
mod app;

#[cfg(windows)]
fn main() {
    app::run();
}

/// Console fallback for non-Windows platforms (useful for development):
/// prints one usage snapshot and exits.
#[cfg(not(windows))]
fn main() {
    let cfg = config::Config::load();
    let mut st = state::AppState::new(cfg.clone());
    let out = st.apply(usage::fetch(&cfg));
    println!("{}", out.tooltip);
}
