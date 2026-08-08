use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::i18n::Lang;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// "auto", "pt" or "en".
    pub language: String,
    /// How often to poll Codex usage, in seconds.
    pub poll_interval_secs: u64,
    /// Timeout for one app-server request cycle.
    pub request_timeout_secs: u64,
    /// Send a Windows notification when usage crosses these percentages.
    pub notify_thresholds: Vec<u8>,
    /// Notify when the focused quota window resets.
    pub notify_on_reset: bool,
    /// Tray icon turns yellow at this percent used.
    pub icon_yellow_at: u8,
    /// Tray icon turns red at this percent used.
    pub icon_red_at: u8,
    /// Codex CLI command or full executable path.
    pub codex_command: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            poll_interval_secs: 180,
            request_timeout_secs: 20,
            notify_thresholds: vec![50, 80, 95],
            notify_on_reset: true,
            icon_yellow_at: 60,
            icon_red_at: 85,
            codex_command: "codex".into(),
        }
    }
}

impl Config {
    pub fn dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codex-usage-tray")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.json")
    }

    /// Loads the config, creating a default file on first run.
    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => {
                let cfg = Self::default();
                let _ = std::fs::create_dir_all(Self::dir());
                if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                    let _ = std::fs::write(&path, json);
                }
                cfg
            }
        }
    }

    pub fn lang(&self) -> Lang {
        match self.language.as_str() {
            "pt" => Lang::Pt,
            "en" => Lang::En,
            _ => detect_lang(),
        }
    }
}

#[cfg(windows)]
fn detect_lang() -> Lang {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    const LANG_PORTUGUESE: u16 = 0x16;
    let id = unsafe { GetUserDefaultUILanguage() };
    if (id & 0x3ff) == LANG_PORTUGUESE {
        Lang::Pt
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn detect_lang() -> Lang {
    if std::env::var("LANG").unwrap_or_default().starts_with("pt") {
        Lang::Pt
    } else {
        Lang::En
    }
}
