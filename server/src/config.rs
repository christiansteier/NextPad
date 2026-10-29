//! Konfiguration aus Umgebungsvariablen und API-Key-Datei.
use std::{fs, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub base_path: String,
    pub api_keys: Vec<Vec<u8>>,
    /// Moodle-Adresse je Schlüssel (zweite Spalte der Schlüsseldatei), für Profilbilder
    pub api_key_moodle: Vec<Option<String>>,
    pub api_allowed: Vec<ipnet::IpNet>,
    pub trusted_proxies: Vec<ipnet::IpNet>,
    /// Erlaubte Origins für WebSocket (eigener Host ist immer erlaubt)
    pub allowed_origins: Vec<String>,
    pub frame_ancestors: String,
    pub api_version: String,
    pub trust_proxy: bool,
    pub allow_free_pads: bool,
    /// Eingeschaltete Addons, Kennungen aus `addons::ALLE`
    pub addons: Vec<&'static str>,
    pub free_pads_per_hour: u32,
    pub cookie_name: String,
    pub author_cookie: String,
    /// Erscheinungsbild: "colibris" (Standard) oder "hsd"
    pub theme: String,
    /// Verzeichnis mit logo.png und font.ttf für das Design (außerhalb des Repos, Marke und Hausschrift)
    pub theme_dir: Option<PathBuf>,
    /// Name der Einrichtung im Fuß der Startseite
    pub site_name: String,
    /// Mailadressen (klein geschrieben) mit Admin-Recht zusätzlich zur Moodle-Administration
    pub admins: Vec<String>,
    /// Öffentliche Adresse inklusive Basis-Pfad (für LTI-Rücksprung), z. B. https://host/np
    pub public_url: Option<String>,
    /// Moodle-Instanzen für die Anmeldung per LTI 1.3
    pub lti: Vec<crate::lti::Platform>,
    /// Cookie der Anmeldung über Moodle
    pub login_cookie: String,
    /// Links im Fuß der Startseite: (Beschriftung, Adresse)
    pub footer_links: Vec<(String, String)>,
    /// Aufbewahrung freier Pads: off, dry-run, on
    pub retention: crate::retention::Mode,
    pub retention_days: i64,
    pub notice_days: i64,
    /// SMTP-Adresse für lettre, z. B. smtp://127.0.0.1:25 oder smtps://user:pass@host:465
    pub smtp_url: Option<String>,
    pub mail_from: Option<String>,
    /// Tage, für die die Zeitleiste jeden Einzelschritt zeigt; danach ein Stand je Tag
    pub history_days: i64,
    /// Bildnachweis für das Hintergrundbild der Startseite (background.jpg im Design-Verzeichnis)
    pub background_credit: Option<String>,
    pub background_credit_url: Option<String>,
}

/// Einstellung `NEXTPAD_<name>`.
pub fn var(name: &str) -> Option<String> {
    std::env::var(format!("NEXTPAD_{name}")).ok()
}

fn env(name: &str, default: &str) -> String {
    var(name).unwrap_or_else(|| default.to_string())
}

/// Datenbankdatei im Datenverzeichnis.
pub fn db_path(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("nextpad.sqlite")
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let data_dir = PathBuf::from(env("DATA", "./data"));
        let keys_file = PathBuf::from(env("API_KEYS_FILE",
            &data_dir.join("api-keys.txt").to_string_lossy(),
        ));
        // Zeile: <schlüssel> [<moodle-basis-url>]
        let entries: Vec<(Vec<u8>, Option<String>)> = match fs::read_to_string(&keys_file) {
            Ok(s) => s
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| {
                    let mut it = l.split_whitespace();
                    let k = it.next().unwrap_or("").as_bytes().to_vec();
                    let url = it.next().map(|u| u.trim_end_matches('/').to_string()).filter(|u| u.starts_with("https://") || u.starts_with("http://"));
                    (k, url)
                })
                .collect(),
            Err(e) => anyhow::bail!("API-Key-Datei {} nicht lesbar: {e}", keys_file.display()),
        };
        let api_keys: Vec<Vec<u8>> = entries.iter().map(|e| e.0.clone()).collect();
        let api_key_moodle: Vec<Option<String>> = entries.into_iter().map(|e| e.1).collect();
        anyhow::ensure!(!api_keys.is_empty(), "API-Key-Datei {} enthält keinen Schlüssel", keys_file.display());
        for k in &api_keys {
            anyhow::ensure!(k.len() >= 32, "API-Key zu kurz (mindestens 32 Zeichen)");
        }
        let mut base_path = env("BASE_PATH", "");
        if base_path.ends_with('/') {
            base_path.pop();
        }
        let lti = crate::lti::load(&PathBuf::from(env("LTI_FILE", &data_dir.join("lti-platforms.json").to_string_lossy())))?;
        let public_url = var("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string()).filter(|u| !u.is_empty());
        anyhow::ensure!(lti.is_empty() || public_url.as_deref().map(|u| u.starts_with("https://")).unwrap_or(false), "LTI braucht NEXTPAD_PUBLIC_URL mit https");
        // NEXTPAD_FOOTER_LINKS="Datenschutz|https://…;Impressum|https://…"
        let footer_links = env("FOOTER_LINKS", "").split(';').filter_map(|e| {
            let (l, u) = e.split_once('|')?;
            let (l, u) = (l.trim(), u.trim());
            (!l.is_empty() && u.starts_with("https://")).then(|| (l.to_string(), u.to_string()))
        }).collect();
        Ok(Config {
            public_url,
            lti,
            login_cookie: env("LOGIN_COOKIE", "nextpad_login"),
            footer_links,
            retention: crate::retention::Mode::parse(&env("RETENTION", "off")),
            retention_days: env("RETENTION_DAYS", "365").parse().unwrap_or(365).max(30),
            notice_days: env("NOTICE_DAYS", "14").parse().unwrap_or(14).clamp(1, 60),
            // Adresse direkt oder aus Datei (systemd LoadCredential), damit Zugangsdaten nicht in der Umgebung stehen
            smtp_url: var("SMTP_URL_FILE").and_then(|f| fs::read_to_string(f).ok()).or_else(|| var("SMTP_URL")).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            history_days: env("HISTORY_DAYS", "30").parse().unwrap_or(30).max(1),
            mail_from: var("MAIL_FROM").filter(|s| s.contains('@')),
            background_credit: var("BACKGROUND_CREDIT").filter(|s| !s.trim().is_empty()),
            background_credit_url: var("BACKGROUND_CREDIT_URL").filter(|s| s.starts_with("https://")),
            bind: env("BIND", "127.0.0.1:5090").parse()?,
            data_dir,
            base_path,
            api_keys,
            api_key_moodle,
            api_allowed: crate::net::parse_list(&env("API_ALLOWED", "127.0.0.1,::1")),
            trusted_proxies: crate::net::parse_list(&env("TRUSTED_PROXIES", "127.0.0.1,::1")),
            allowed_origins: env("ALLOWED_ORIGINS", "").split(',').map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).collect(),
            frame_ancestors: env("FRAME_ANCESTORS", "'self'"),
            api_version: env("API_VERSION", "1.2.15"),
            trust_proxy: env("TRUST_PROXY", "false") == "true",
            allow_free_pads: env("ALLOW_FREE_PADS", "false") == "true",
            addons: crate::addons::auswahl(var("ADDONS").as_deref())?,
            free_pads_per_hour: env("FREE_PADS_PER_HOUR", "20").parse().unwrap_or(20),
            author_cookie: env("AUTHOR_COOKIE", "nextpad_author"),
            theme: env("THEME", "colibris"),
            theme_dir: var("THEME_DIR").filter(|s| !s.is_empty()).map(PathBuf::from),
            site_name: env("SITE_NAME", ""),
            admins: env("ADMINS", "").split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| s.contains('@')).collect(),
            cookie_name: env("COOKIE_NAME", "sessionID"),
        })
    }
}
