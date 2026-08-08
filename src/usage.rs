use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::path::{Path, PathBuf};

use crate::config::Config;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    let stderr = child.stderr.take().ok_or("could not open codex stderr")?;

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

    // Always drain stderr so a noisy subprocess cannot block on a full pipe.
    // Keeping it also lets us expose the actual Codex/cmd error instead of
    // misreporting every early process exit as a timeout.
    let stderr_handle = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut text);
        text
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
    let stderr_text = stderr_handle.join().unwrap_or_default();

    match result {
        Err(e) if !stderr_text.trim().is_empty() => {
            Err(format!("{e}: {}", compact_stderr(&stderr_text)))
        }
        other => other,
    }
}

fn spawn_app_server(cfg: &Config) -> Result<Child, String> {
    let command = cfg.codex_command.trim();
    if command.is_empty() {
        return Err("codex_command is empty".into());
    }

    #[cfg(windows)]
    let mut cmd = windows_codex_command(command)?;

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new(command);
        c.args(["app-server", "--stdio"]);
        c
    };

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "failed to start `{command} app-server --stdio`: {e}. Is Codex installed and accessible?"
            )
        })
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    // GUI tray applications should never flash a console window during the
    // periodic app-server query. This applies both to codex.exe and cmd.exe
    // when an npm/bun .cmd shim has to be used.
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
fn windows_codex_command(requested: &str) -> Result<Command, String> {
    let resolved = resolve_windows_codex(requested);
    let lower = resolved.to_string_lossy().to_ascii_lowercase();

    if lower.ends_with(".cmd") || lower.ends_with(".bat") {
        // cmd.exe needs the outer quote pair when the script path itself is
        // quoted. This form works for npm/bun shims even when the path has
        // spaces: cmd /D /S /C ""C:\path\codex.cmd" app-server --stdio"
        let invocation = format!(
            "\"\"{}\" app-server --stdio\"",
            resolved.to_string_lossy()
        );
        let mut c = Command::new("cmd.exe");
        c.args(["/D", "/S", "/C"]).arg(invocation);
        hide_console_window(&mut c);
        return Ok(c);
    }

    if resolved.is_file() || lower.ends_with(".exe") {
        let mut c = Command::new(&resolved);
        c.args(["app-server", "--stdio"]);
        hide_console_window(&mut c);
        return Ok(c);
    }

    // Final compatibility fallback for custom commands/aliases. `cmd.exe`
    // handles PATHEXT resolution for commands such as an npm `codex.cmd` that
    // is actually present in the inherited PATH.
    let raw = resolved.to_string_lossy();
    let invocation = if raw.chars().any(char::is_whitespace) {
        format!("\"{raw}\" app-server --stdio")
    } else {
        format!("{raw} app-server --stdio")
    };
    let mut c = Command::new("cmd.exe");
    c.args(["/D", "/C"]).arg(invocation);
    hide_console_window(&mut c);
    Ok(c)
}

#[cfg(windows)]
fn resolve_windows_codex(requested: &str) -> PathBuf {
    let clean = requested.trim().trim_matches('"');

    // An explicit path in config always wins.
    let explicit = PathBuf::from(clean);
    if clean != "codex" && explicit.is_file() {
        return explicit;
    }

    // First respect the PATH inherited by this GUI process.
    if clean.eq_ignore_ascii_case("codex") {
        if let Some(path) = find_windows_command_on_path("codex") {
            return path;
        }

        // Official standalone installer (current documented Windows install).
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            let path = PathBuf::from(local_app_data)
                .join("Programs")
                .join("OpenAI")
                .join("Codex")
                .join("bin")
                .join("codex.exe");
            if path.is_file() {
                return path;
            }
        }

        // The standalone install keeps a stable `current` junction under
        // CODEX_HOME (or ~/.codex by default). Try both current layouts.
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")));
        if let Some(home) = codex_home {
            for relative in [
                Path::new("packages")
                    .join("standalone")
                    .join("current")
                    .join("bin")
                    .join("codex.exe"),
                Path::new("packages")
                    .join("standalone")
                    .join("current")
                    .join("codex.exe"),
            ] {
                let path = home.join(relative);
                if path.is_file() {
                    return path;
                }
            }
        }

        // npm global binaries on Windows normally live here.
        if let Some(app_data) = env::var_os("APPDATA") {
            let path = PathBuf::from(app_data).join("npm").join("codex.cmd");
            if path.is_file() {
                return path;
            }
        }

        // bun global installs are another supported package-manager route.
        if let Some(user_profile) = env::var_os("USERPROFILE") {
            let bin = PathBuf::from(user_profile).join(".bun").join("bin");
            for name in ["codex.exe", "codex.cmd"] {
                let path = bin.join(name);
                if path.is_file() {
                    return path;
                }
            }
        }
    }

    PathBuf::from(clean)
}

#[cfg(windows)]
fn find_windows_command_on_path(name: &str) -> Option<PathBuf> {
    let path_value = env::var_os("PATH")?;
    let pathext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let extensions = pathext
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_ascii_lowercase())
        .collect::<Vec<_>>();

    for dir in env::split_paths(&path_value) {
        let plain = dir.join(name);
        if plain.is_file() {
            return Some(plain);
        }
        for ext in &extensions {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
            let candidate_upper = dir.join(format!("{name}{}", ext.to_ascii_uppercase()));
            if candidate_upper.is_file() {
                return Some(candidate_upper);
            }
        }
    }
    None
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
            return Err(format!(
                "codex app-server timed out waiting for response id {wanted_id}"
            ));
        }
        let remaining = deadline.saturating_duration_since(now);
        let line = match rx.recv_timeout(remaining) {
            Ok(line) => line?,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "codex app-server timed out waiting for response id {wanted_id}"
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "codex app-server closed stdout before response id {wanted_id}"
                ));
            }
        };
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

fn compact_stderr(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    if joined.chars().count() <= 500 {
        joined
    } else {
        let cut: String = joined.chars().take(499).collect();
        format!("{cut}…")
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
        plan_type: rate
            .get("planType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        limit_id: rate
            .get("limitId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        reset_credits_available,
    })
}

fn parse_window(value: Option<&Value>) -> Option<WindowUsage> {
    let value = value?.as_object()?;
    let utilization = value
        .get("usedPercent")
        .and_then(Value::as_f64)
        .or_else(|| {
            value
                .get("usedPercent")
                .and_then(Value::as_i64)
                .map(|n| n as f64)
        })?
        .clamp(0.0, 100.0);
    let duration_minutes = value.get("windowDurationMins").and_then(Value::as_u64);
    let resets_at = value
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(|ts| Utc.timestamp_opt(ts, 0).single());
    Some(WindowUsage {
        utilization,
        duration_minutes,
        resets_at,
    })
}

fn parse_credits(value: Option<&Value>) -> Option<Credits> {
    let value = value?.as_object()?;
    Some(Credits {
        has_credits: value
            .get("hasCredits")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        unlimited: value
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        balance: value
            .get("balance")
            .and_then(Value::as_str)
            .map(str::to_owned),
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

    #[test]
    fn compact_stderr_flattens_lines() {
        assert_eq!(compact_stderr("first\r\nsecond\n"), "first | second");
    }
}
