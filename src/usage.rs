use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct WindowUsage {
    pub utilization: f64,
    pub duration_minutes: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct Credits {
    pub has_credits: bool,
    pub unlimited: bool,
    pub balance: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub primary: Option<WindowUsage>,
    pub secondary: Option<WindowUsage>,
    pub credits: Option<Credits>,
    pub plan_type: Option<String>,
    pub limit_id: Option<String>,
    pub reset_credits_available: Option<u64>,
}

impl Snapshot {
    pub fn focus_window(&self) -> Option<&WindowUsage> {
        [self.primary.as_ref(), self.secondary.as_ref()]
            .into_iter()
            .flatten()
            .min_by_key(|w| w.duration_minutes.unwrap_or(u64::MAX))
            .or(self.primary.as_ref())
            .or(self.secondary.as_ref())
    }

    pub fn windows(&self) -> impl Iterator<Item = &WindowUsage> {
        [self.primary.as_ref(), self.secondary.as_ref()]
            .into_iter()
            .flatten()
    }
}

pub fn fetch(cfg: &Config) -> Result<Snapshot, String> {
    let mut child = spawn_app_server(cfg)?;
    let mut stdin = child.stdin.take().ok_or("could not open codex stdin")?;
    let stdout = child.stdout.take().ok_or("could not open codex stdout")?;

    let (tx, rx) = mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("reading codex app-server: {e}")));
                    break;
                }
            }
        }
    });

    let timeout = Duration::from_secs(cfg.request_timeout_secs.max(5));
    let result = (|| {
        write_json_line(
            &mut stdin,
            &serde_json::json!({
                "method": "initialize",
                "id": 1,
                "params": {
                    "clientInfo": {
                        "name": "codex_usage_tray",
                        "title": "Codex Usage Tray",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
        )?;
        wait_for_id(&rx, 1, timeout)?;

        write_json_line(
            &mut stdin,
            &serde_json::json!({ "method": "initialized", "params": {} }),
        )?;
        write_json_line(
            &mut stdin,
            &serde_json::json!({ "method": "account/rateLimits/read", "id": 2 }),
        )?;

        let result = wait_for_id(&rx, 2, timeout)?;
        snapshot_from_result(&result)
    })();

    let _ = child.kill();
    let _ = child.wait();
    result
}

fn spawn_app_server(cfg: &Config) -> Result<Child, String> {
    let command = cfg.codex_command.trim();
    if command.is_empty() {
        return Err("codex_command is empty".into());
    }

    #[cfg(windows)]
    let mut cmd = {
        let escaped = command.replace('"', "\"");
        let mut c = Command::new("cmd");
        c.args(["/D", "/S", "/C"])
            .arg(format!("\"{escaped}\" app-server --stdio"));
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new(command);
        c.args(["app-server", "--stdio"]);
        c
    };

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "failed to start `{command} app-server --stdio`: {e}. Is Codex installed and on PATH?"
            )
        })
}

fn write_json_line(stdin: &mut ChildStdin, value: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, value).map_err(|e| format!("encode request: {e}"))?;
    stdin
        .write_all(b"\n")
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("write to codex app-server: {e}"))
}

fn wait_for_id(
    rx: &mpsc::Receiver<Result<String, String>>,
    wanted_id: i64,
    timeout: Duration,
) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!("codex app-server timed out waiting for response id {wanted_id}"));
        }
        let remaining = deadline.saturating_duration_since(now);
        let line = rx
            .recv_timeout(remaining)
            .map_err(|_| format!("codex app-server timed out waiting for response id {wanted_id}"))??;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if value.get("id").and_then(Value::as_i64) != Some(wanted_id) {
            continue;
        }
        if let Some(err) = value.get("error") {
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown app-server error");
            return Err(format!("codex app-server: {message}"));
        }
        return value
            .get("result")
            .cloned()
            .ok_or_else(|| format!("codex app-server response {wanted_id} had no result"));
    }
}

fn snapshot_from_result(result: &Value) -> Result<Snapshot, String> {
    let rate = result
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .and_then(|m| m.get("codex"))
        .filter(|v| !v.is_null())
        .or_else(|| result.get("rateLimits"))
        .ok_or("rateLimits missing from account/rateLimits/read")?;

    let reset_credits_available = result
        .get("rateLimitResetCredits")
        .and_then(|v| v.get("availableCount"))
        .and_then(Value::as_u64);

    Ok(Snapshot {
        primary: parse_window(rate.get("primary")),
        secondary: parse_window(rate.get("secondary")),
        credits: parse_credits(rate.get("credits")),
        plan_type: rate.get("planType").and_then(Value::as_str).map(str::to_owned),
        limit_id: rate.get("limitId").and_then(Value::as_str).map(str::to_owned),
        reset_credits_available,
    })
}

fn parse_window(value: Option<&Value>) -> Option<WindowUsage> {
    let value = value?.as_object()?;
    let utilization = value
        .get("usedPercent")
        .and_then(Value::as_f64)
        .or_else(|| value.get("usedPercent").and_then(Value::as_i64).map(|n| n as f64))?
        .clamp(0.0, 100.0);
    let duration_minutes = value.get("windowDurationMins").and_then(Value::as_u64);
    let resets_at = value
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(|ts| Utc.timestamp_opt(ts, 0).single());
    Some(WindowUsage { utilization, duration_minutes, resets_at })
}

fn parse_credits(value: Option<&Value>) -> Option<Credits> {
    let value = value?.as_object()?;
    Some(Credits {
        has_credits: value.get("hasCredits").and_then(Value::as_bool).unwrap_or(false),
        unlimited: value.get("unlimited").and_then(Value::as_bool).unwrap_or(false),
        balance: value.get("balance").and_then(Value::as_str).map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_current_codex_shape() {
        let v = json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 25, "windowDurationMins": 300, "resetsAt": 1779459394i64 },
                "secondary": { "usedPercent": 18, "windowDurationMins": 10080, "resetsAt": 1779826837i64 },
                "credits": { "hasCredits": true, "unlimited": false, "balance": "766.76" },
                "planType": "pro"
            },
            "rateLimitResetCredits": { "availableCount": 2 }
        });
        let s = snapshot_from_result(&v).unwrap();
        assert_eq!(s.primary.as_ref().unwrap().duration_minutes, Some(300));
        assert_eq!(s.secondary.as_ref().unwrap().duration_minutes, Some(10080));
        assert_eq!(s.focus_window().unwrap().utilization, 25.0);
        assert_eq!(s.credits.unwrap().balance.as_deref(), Some("766.76"));
        assert_eq!(s.reset_credits_available, Some(2));
    }

    #[test]
    fn prefers_codex_multi_bucket_snapshot() {
        let v = json!({
            "rateLimits": { "limitId": "legacy", "primary": { "usedPercent": 99, "windowDurationMins": 300 } },
            "rateLimitsByLimitId": { "codex": { "limitId": "codex", "primary": { "usedPercent": 12, "windowDurationMins": 300 }, "secondary": null } }
        });
        let s = snapshot_from_result(&v).unwrap();
        assert_eq!(s.limit_id.as_deref(), Some("codex"));
        assert_eq!(s.primary.unwrap().utilization, 12.0);
    }

    #[test]
    fn weekly_only_primary_is_supported() {
        let v = json!({ "rateLimits": { "limitId": "codex", "primary": { "usedPercent": 22, "windowDurationMins": 10080 }, "secondary": null } });
        let s = snapshot_from_result(&v).unwrap();
        assert_eq!(s.focus_window().unwrap().duration_minutes, Some(10080));
    }
}
