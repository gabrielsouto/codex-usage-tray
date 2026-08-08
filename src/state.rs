use chrono::{DateTime, Local, Utc};

use crate::config::Config;
use crate::i18n::{tr, Lang};
use crate::usage::{Snapshot, WindowUsage};

pub struct Output {
    pub tooltip: String,
    /// Percent used for the icon; `None` means error (gray icon).
    pub utilization: Option<f64>,
    /// (title, body) pairs to show as Windows notifications.
    pub notifications: Vec<(String, String)>,
}

pub struct AppState {
    cfg: Config,
    lang: Lang,
    last_util: Option<f64>,
    last_duration: Option<u64>,
}

impl AppState {
    pub fn new(cfg: Config) -> Self {
        let lang = cfg.lang();
        Self {
            cfg,
            lang,
            last_util: None,
            last_duration: None,
        }
    }

    pub fn apply(&mut self, res: Result<Snapshot, String>) -> Output {
        match res {
            Err(e) => Output {
                tooltip: truncate(&format!("Codex: {}\n{e}", tr(self.lang, "error")), 126),
                utilization: None,
                notifications: Vec::new(),
            },
            Ok(snap) => self.apply_ok(&snap),
        }
    }

    fn apply_ok(&mut self, snap: &Snapshot) -> Output {
        let lang = self.lang;
        let focus = snap.focus_window();
        let util = focus.map(|w| w.utilization);
        let duration = focus.and_then(|w| w.duration_minutes);
        let mut notifications = Vec::new();

        if let Some(now_u) = util {
            if self.last_duration == duration {
                if let Some(prev) = self.last_util {
                    if self
                        .cfg
                        .notify_thresholds
                        .iter()
                        .any(|&&t| prev < t as f64 && now_u >= t as f64)
                    {
                        notifications.push((
                            tr(lang, "notif_title").to_string(),
                            threshold_text(lang, now_u, focus.unwrap()),
                        ));
                    }
                    if self.cfg.notify_on_reset && now_u + 5.0 < prev {
                        notifications.push((
                            tr(lang, "notif_title").to_string(),
                            reset_text(lang, focus.unwrap()),
                        ));
                    }
                }
            }
            self.last_util = Some(now_u);
            self.last_duration = duration;
        }

        let mut windows: Vec<&WindowUsage> = snap.windows().collect();
        windows.sort_by_key(|w| w.duration_minutes.unwrap_or(u64::MAX));
        let mut lines: Vec<String> = windows.into_iter().map(|w| window_line(lang, w)).collect();

        if lines.is_empty() {
            lines.push(match lang {
                Lang::Pt => "Codex: limites indisponíveis".into(),
                Lang::En => "Codex: limits unavailable".into(),
            });
        }

        if let Some(c) = &snap.credits {
            if c.unlimited {
                lines.push(format!("{}: {}", tr(lang, "credits"), tr(lang, "unlimited")));
            } else if c.has_credits {
                if let Some(balance) = &c.balance {
                    lines.push(format!("{}: {balance}", tr(lang, "credits")));
                }
            }
        }
        if let Some(n) = snap.reset_credits_available.filter(|&n| n > 0) {
            lines.push(format!("{}: {n}", tr(lang, "reset_credits")));
        }

        Output {
            tooltip: truncate(&lines.join("\n"), 126),
            utilization: util.or(Some(0.0)),
            notifications,
        }
    }
}

fn window_line(lang: Lang, w: &WindowUsage) -> String {
    let mut line = format!("{}: {:.0}% {}", window_label(w.duration_minutes), w.utilization, tr(lang, "used"));
    if let Some(r) = w.resets_at {
        line.push_str(&format!(" · {} {}", tr(lang, "resets"), fmt_reset(r)));
    }
    line
}

fn threshold_text(lang: Lang, util: f64, w: &WindowUsage) -> String {
    let label = window_label(w.duration_minutes);
    let reset = w.resets_at.map(fmt_reset);
    match (lang, reset) {
        (Lang::Pt, Some(r)) => format!("Você já usou {util:.0}% da janela {label}. Reseta {r}."),
        (Lang::Pt, None) => format!("Você já usou {util:.0}% da janela {label}."),
        (Lang::En, Some(r)) => format!("You've used {util:.0}% of the {label} window. Resets {r}."),
        (Lang::En, None) => format!("You've used {util:.0}% of the {label} window."),
    }
}

fn reset_text(lang: Lang, w: &WindowUsage) -> String {
    let label = window_label(w.duration_minutes);
    match lang {
        Lang::Pt => format!("Nova janela {label} — cota novamente disponível."),
        Lang::En => format!("New {label} window — quota available again."),
    }
}

pub fn window_label(minutes: Option<u64>) -> String {
    match minutes {
        Some(300) => "5h".into(),
        Some(10_080) => "7d".into(),
        Some(m) if m % 1_440 == 0 => format!("{}d", m / 1_440),
        Some(m) if m % 60 == 0 => format!("{}h", m / 60),
        Some(m) => format!("{m}min"),
        None => "quota".into(),
    }
}

pub fn fmt_reset(at: DateTime<Utc>) -> String {
    fmt_reset_from(at, Utc::now())
}

fn fmt_reset_from(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    let mins = (at - now).num_minutes().max(0);
    let left = if mins >= 1_440 {
        format!("{}d{}h", mins / 1_440, (mins % 1_440) / 60)
    } else if mins >= 60 {
        format!("{}h{:02}", mins / 60, mins % 60)
    } else {
        format!("{mins}min")
    };
    let time = if mins >= 1_440 {
        local.format("%d/%m %H:%M").to_string()
    } else {
        local.format("%H:%M").to_string()
    };
    format!("{time} ({left})")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Credits, Snapshot, WindowUsage};
    use chrono::TimeZone;

    fn snap(util: f64) -> Result<Snapshot, String> {
        Ok(Snapshot {
            primary: Some(WindowUsage { utilization: util, duration_minutes: Some(300), resets_at: Some(Utc.with_ymd_and_hms(2026, 8, 7, 18, 0, 0).unwrap()) }),
            secondary: Some(WindowUsage { utilization: 12.0, duration_minutes: Some(10_080), resets_at: None }),
            credits: Some(Credits { has_credits: true, unlimited: false, balance: Some("12.34".into()) }),
            plan_type: Some("plus".into()),
            limit_id: Some("codex".into()),
            reset_credits_available: None,
        })
    }

    fn state() -> AppState {
        let mut cfg = Config::default();
        cfg.language = "en".into();
        AppState::new(cfg)
    }

    #[test]
    fn no_notification_on_first_sample() {
        let mut st = state();
        let out = st.apply(snap(90.0));
        assert!(out.notifications.is_empty());
        assert_eq!(out.utilization, Some(90.0));
    }

    #[test]
    fn notifies_once_when_crossing_threshold() {
        let mut st = state();
        st.apply(snap(45.0));
        let out = st.apply(snap(55.0));
        assert_eq!(out.notifications.len(), 1);
        assert!(out.notifications[0].1.contains("55%"));
        assert!(st.apply(snap(60.0)).notifications.is_empty());
    }

    #[test]
    fn reset_detected_when_usage_drops() {
        let mut st = state();
        st.apply(snap(85.0));
        let out = st.apply(snap(2.0));
        assert_eq!(out.notifications.len(), 1);
    }

    #[test]
    fn tooltip_fits_windows_limit() {
        let mut st = state();
        let out = st.apply(snap(33.0));
        assert!(out.tooltip.chars().count() <= 126);
        assert!(out.tooltip.contains("5h: 33%"));
        assert!(out.tooltip.contains("7d: 12%"));
    }

    #[test]
    fn labels_are_duration_based() {
        assert_eq!(window_label(Some(300)), "5h");
        assert_eq!(window_label(Some(10_080)), "7d");
        assert_eq!(window_label(Some(1_440)), "1d");
    }
}
