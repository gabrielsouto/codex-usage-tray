#[cfg(windows)]
pub fn toast(title: &str, body: &str) {
    use tauri_winrt_notification::{Duration, Toast};
    let _ = Toast::new(Toast::POWERSHELL_APP_ID)
        .title(title)
        .text1(body)
        .duration(Duration::Short)
        .show();
}

#[cfg(not(windows))]
pub fn toast(title: &str, body: &str) {
    eprintln!("[notification] {title}: {body}");
}
