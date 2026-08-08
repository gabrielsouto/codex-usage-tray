#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Lang {
    Pt,
    En,
}

pub fn tr(lang: Lang, key: &str) -> &'static str {
    use Lang::*;
    match (lang, key) {
        (Pt, "loading") => "Codex Usage — carregando…",
        (En, "loading") => "Codex Usage — loading…",
        (Pt, "refresh") => "Atualizar agora",
        (En, "refresh") => "Refresh now",
        (Pt, "open_usage") => "Abrir página de uso do Codex",
        (En, "open_usage") => "Open Codex usage page",
        (Pt, "open_config") => "Abrir configurações",
        (En, "open_config") => "Open settings",
        (Pt, "quit") => "Sair",
        (En, "quit") => "Quit",
        (Pt, "used") => "usado",
        (En, "used") => "used",
        (Pt, "resets") => "reseta",
        (En, "resets") => "resets",
        (Pt, "error") => "erro ao consultar uso",
        (En, "error") => "error fetching usage",
        (Pt, "notif_title") => "Uso do Codex",
        (En, "notif_title") => "Codex usage",
        (Pt, "credits") => "créditos",
        (En, "credits") => "credits",
        (Pt, "unlimited") => "ilimitados",
        (En, "unlimited") => "unlimited",
        (Pt, "reset_credits") => "resets disponíveis",
        (En, "reset_credits") => "resets available",
        _ => "",
    }
}
