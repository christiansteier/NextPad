//! Editorseite `/p/<padID>`, WebSocket-Upgrade, Export, Statusseite.
use crate::sperre::Sperre;
use crate::texte::{self, Sprache};
use crate::{ids, store::Pad, sync, AppState};
use axum::{
    extract::{ws::WebSocketUpgrade, ConnectInfo, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

pub const EDITOR_JS: &str = include_str!("../static/editor.js");
pub const EDITOR_CSS: &str = include_str!("../static/editor.css");
pub const ICONS_WOFF: &[u8] = include_bytes!("../static/icons.woff");
pub const FOND_JPG: &[u8] = include_bytes!("../static/fond.jpg");
pub const LOGO_SVG: &str = include_str!("../static/logo.svg");
pub const FAVICON_PNG: &[u8] = include_bytes!("../static/favicon-32.png");
pub const TOUCH_ICON_PNG: &[u8] = include_bytes!("../static/apple-touch-icon.png");
pub const QUICKSAND_TTF: &[u8] = include_bytes!("../static/quicksand.ttf");
pub const THEME_HSD_CSS: &str = include_str!("../static/theme-hsd.css");
/// Inhaltskennung des Bundles für versionierte URLs: Browser-Caches zeigen nach jedem Bau sofort die neue Fassung.
pub fn bundle_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for b in EDITOR_JS.bytes().chain(EDITOR_CSS.bytes()).chain(THEME_HSD_CSS.bytes()) { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        format!("{h:016x}")
    })
}

pub const PALETTE: [&str; 16] = [
    "#ffc7c7", "#fff1c7", "#e3ffc7", "#c7ffd5", "#c7ffff", "#c7d5ff", "#e3c7ff", "#ffc7f1",
    "#ffe0c7", "#f5ffc7", "#c7ffe8", "#c7e8ff", "#d5c7ff", "#ffc7e0", "#ededed", "#d2f7a1",
];

pub struct Access {
    pub pad: Pad,
    pub readonly: bool,
    pub author: Option<(String, String)>, // (id, name)
    pub avatar: Option<String>,
    /// true: freies Pad (Gast), Identität über das eigene Autoren-Cookie
    pub free: bool,
    /// zu setzendes Autoren-Cookie (nur bei freien Pads, wenn noch keins vorhanden)
    pub set_cookie: Option<String>,
    /// true: über Moodle (LTI) angemeldet, Name und Bild kommen aus Moodle
    pub login: bool,
}

/// Über Moodle (LTI) angemeldete Person aus dem Anmelde-Cookie.
pub fn login_author(state: &AppState, headers: &HeaderMap) -> Option<crate::store::Author> {
    let now_s = ids::now_ms() / 1000;
    cookie_values(headers, &state.config.login_cookie).into_iter().find_map(|t| state.store.author_by_login(&t, now_s).ok().flatten())
}

/// Ursprünge (Schema und Host) der bekannten Moodle-Instanzen, für Profilbilder in der CSP.
pub(crate) fn moodle_origins(state: &AppState) -> String {
    let urls = state.config.api_key_moodle.iter().flatten().cloned().chain(state.config.lti.iter().map(|p| p.moodle()));
    urls.filter_map(|u| { let (scheme, rest) = u.split_once("://")?; Some(format!("{scheme}://{}", rest.split('/').next()?)) })
        .collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" ")
}

/// Gastautor aus dem eigenen Cookie (geheimes Token, nicht die öffentliche Autorenkennung).
/// Legt nur bei `create` einen neuen an (Editorseite); sonst ohne Cookie: kein Autor.
/// Gastautor: Kennung, Name, neu zu setzendes Cookie
type Gast = (String, String, Option<String>);

fn guest_author(state: &AppState, headers: &HeaderMap, create: bool) -> Result<Option<Gast>, (StatusCode, &'static str)> {
    let path = if state.config.base_path.is_empty() { "/".to_string() } else { format!("{}/", state.config.base_path) };
    let cookie = |token: &str| format!("{}={}; Path={}; Max-Age=31536000; HttpOnly; Secure; SameSite=Lax", state.config.author_cookie, token, path);
    for v in cookie_values(headers, &state.config.author_cookie) {
        if let Ok(Some(a)) = state.store.author_by_guest_token(&v) { return Ok(Some((a.id, a.name, None))); }
    }
    if !create { return Ok(None); }
    let a = state.store.create_author("Gast", None).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "zugang.autor_fehler"))?;
    let token = state.store.create_guest_token(&a.id).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "zugang.autor_fehler"))?;
    Ok(Some((a.id, a.name, Some(cookie(&token)))))
}

/// Zulässige Namen freier Pads: wie Etherpad begrenzt, ohne Steuer- und Trennzeichen.
pub fn valid_free_pad_name(n: &str) -> bool {
    let len = n.chars().count();
    (1..=50).contains(&len) && !n.starts_with("g.") && !n.starts_with("r.") && !n.starts_with(' ') && !n.ends_with(' ')
        && n.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')' | ','))
}

pub fn cookie_values(headers: &HeaderMap, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for c in headers.get_all(header::COOKIE) {
        let Ok(s) = c.to_str() else { continue };
        for part in s.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(&format!("{name}=")) {
                out.extend(v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()));
            }
        }
    }
    out
}

/// Prüft Pad, Session und Lesemodus. Fehler: (Status, Schlüssel im Textkatalog).
pub fn check_access(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Access, (StatusCode, &'static str)> { check_access_ext(state, headers, id, false) }

pub fn check_access_ext(state: &AppState, headers: &HeaderMap, id: &str, create_guest: bool) -> Result<Access, (StatusCode, &'static str)> {
    let (pad, readonly) = if id.starts_with("r.") {
        match state.store.pad_by_readonly(id) {
            Ok(Some(p)) => (p, true),
            _ => return Err((StatusCode::NOT_FOUND, "zugang.pad_fehlt")),
        }
    } else {
        match state.store.get_pad(id) {
            Ok(Some(p)) => (p, false),
            _ => return Err((StatusCode::NOT_FOUND, "zugang.pad_fehlt")),
        }
    };
    let Some(group) = pad.group_id.clone() else {
        if state.config.allow_free_pads {
            if let Some(a) = login_author(state, headers) {
                return Ok(Access { pad, readonly, author: Some((a.id, a.name)), avatar: a.avatar, free: true, set_cookie: None, login: true });
            }
            return Ok(match guest_author(state, headers, create_guest)? {
                Some((aid, name, set_cookie)) => Access { pad, readonly, author: Some((aid, name)), avatar: None, free: true, set_cookie, login: false },
                None => Access { pad, readonly: true, author: None, avatar: None, free: true, set_cookie: None, login: false },
            });
        }
        return Err((StatusCode::FORBIDDEN, "zugang.ohne_gruppe"));
    };
    let now_s = ids::now_ms() / 1000;
    for sid in cookie_values(headers, &state.config.cookie_name) {
        if let Ok(Some(s)) = state.store.get_session(&sid) {
            if s.group_id == group && s.valid_until > now_s {
                let a = state.store.get_author(&s.author_id).ok().flatten();
                let avatar = a.as_ref().and_then(|a| a.avatar.clone());
                let author = a.map(|a| (a.id, a.name));
                return Ok(Access { pad, readonly, author, avatar, free: false, set_cookie: None, login: false });
            }
        }
    }
    Err((StatusCode::FORBIDDEN, "zugang.keine_sitzung"))
}

/// Basis-Pfad für Seiten ohne Zugriff auf den Zustand (Fehlerseiten); einmal beim Start gesetzt.
pub static BASE_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Fehlerseite in der Sprache der Anfrage; `schluessel` aus dem Textkatalog.
pub(crate) fn fehler(headers: &HeaderMap, status: StatusCode, schluessel: &'static str) -> Response {
    let sp = Sprache::aus_anfrage(headers);
    error_page(sp, status, texte::t(sp, schluessel))
}

/// Ablehnung als Klartext für Skripte, in der Sprache der Anfrage.
pub(crate) fn abgelehnt(headers: &HeaderMap, status: StatusCode, schluessel: &'static str) -> Response {
    (status, texte::t(Sprache::aus_anfrage(headers), schluessel)).into_response()
}

/// Fehlerseite; `msg` ist schon in der Sprache `sp`.
pub(crate) fn error_page(sp: Sprache, status: StatusCode, msg: &str) -> Response {
    let base = BASE_PATH.get().map(String::as_str).unwrap_or("");
    let title = texte::t(sp, match status.as_u16() { 400 => "fehler.titel.400", 403 => "fehler.titel.403", 404 => "fehler.titel.404", 429 => "fehler.titel.429", _ => "fehler.titel.sonst" });
    let icons = icon_links();
    (status, Html(format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light dark"><title>{title} · NextPad</title><link rel="stylesheet" href="{base}/static/start.css?v={v}"></head>
<body><main class="doc"><h1>{title}</h1><p class="notice" role="alert">{msg}</p><p><a class="btn btn-secondary" href="{base}/">{start}</a></p></main></body></html>"#, lang = sp.kuerzel(), msg = html_escape::encode_text(msg), start = texte::t(sp, "fehler.zur_startseite"), v = start_version()))).into_response()
}

pub async fn page(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match check_access_ext(&state, &headers, &id, true) {
        Ok(a) => a,
        Err((s, m)) => return fehler(&headers, s, m),
    };
    let sp = Sprache::aus_anfrage(&headers);
    let (aid, aname) = access.author.clone().unwrap_or_default();
    let color = PALETTE[ids::color_index(&aid, PALETTE.len())];
    let mut owner = false;
    let mut expires = serde_json::Value::Null;
    if access.free {
        let _ = state.store.touch_seen(&access.pad.id, ids::now_ms());
        owner = !aid.is_empty() && state.store.owner_of(&access.pad.id).ok().flatten().as_deref() == Some(aid.as_str());
        if owner && state.config.retention != crate::retention::Mode::Off {
            if let Ok(Some(r)) = state.store.retention_row(&access.pad.id) { expires = json!(r.delete_after.unwrap_or_else(|| crate::retention::expires(&r, state.config.retention_days * 86_400_000))); }
        }
    }
    let cfg = json!({
        "padId": if access.readonly { id.clone() } else { access.pad.id.clone() },
        "urlId": id,
        "readonly": access.readonly,
        "author": { "id": aid, "name": aname, "color": color, "avatar": access.avatar, "guest": access.author.is_none() },
        "basePath": state.config.base_path,
        "palette": PALETTE,
        "free": access.free,
        "login": access.login,
        "owner": owner,
        "restore": may_restore(&state, &access),
        "expires": expires,
        "theme": state.config.theme,
        "logo": if theme_hsd(&state) && has_logo(&state) { serde_json::Value::String(theme_url(&state, "logo.png")) } else { serde_json::Value::Null },
        "logoAlt": crate::look::get(&state).institution(),
        "course": if access.free { None } else { state.store.course_url(&access.pad.id) },
        "lang": sp.kuerzel(),
        "texte": texte::texte_mit(sp, "editor."),
    });
    let base = &state.config.base_path;
    if let Some(addon) = crate::addons::von_pad(&state, &access.pad.id) {
        if !crate::addons::aktiv(&state, addon) { return crate::addons::nicht_aktiv(addon, &headers); }
        let readonly_id = (!access.readonly).then_some(access.pad.readonly_id.as_str());
        let resp = (addon.seite)(crate::addons::Seite { state: &state, headers: &headers, url_id: &id, pad_id: &access.pad.id, free: access.free, readonly_id, cfg });
        return with_cookie(resp, access.set_cookie.clone());
    }
    let theme_link = if theme_hsd(&state) { format!(r#"<link rel="stylesheet" href="{}/static/theme-hsd.css?v={}">"#, state.config.base_path, bundle_version()) } else { String::new() };
    let body_class = if theme_hsd(&state) { "theme-hsd" } else { "theme-colibris" };
    let icons = icon_links();
    let body = format!(
        r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad</title>
<link rel="stylesheet" href="{base}/static/editor.css?v={v}">{theme_link}{look}</head>
<body class="{body_class}"><div id="app"><div id="toolbar" role="toolbar" aria-label="{formatierung}"></div><div id="editorcontainerbox"><div id="editor"></div></div><div id="statusbar"></div></div>
<script type="application/json" id="nextpad-config">{cfg}</script>
<script src="{base}/static/editor.js?v={v}"></script></body></html>"#,
        v = bundle_version(),
        lang = sp.kuerzel(),
        formatierung = texte::t(sp, "editorseite.formatierung"),
        look = look_css(&state),
        cfg = cfg.to_string().replace("</", "<\\/")
    );
    let mut resp = Html(body).into_response();
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: {}; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors {}; base-uri 'none'", moodle_origins(&state), state.config.frame_ancestors);
    let h = resp.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_str(&csp).unwrap_or(HeaderValue::from_static("default-src 'self'")));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(c) = &access.set_cookie { if let Ok(v) = HeaderValue::from_str(c) { h.append(header::SET_COOKIE, v); } }
    resp
}

/// Origin-Prüfung gegen Cross-Site-WebSocket-Hijacking: eigener Host oder ausdrücklich erlaubte Origins.
pub fn origin_ok(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true }; // Nicht-Browser-Clients
    let origin = origin.trim_end_matches('/');
    let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("");
    let own = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")).map(|h| h == host).unwrap_or(false);
    own || state.config.allowed_origins.iter().any(|o| o == origin)
}

pub async fn ws(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    if !origin_ok(&state, &headers) { return abgelehnt(&headers, StatusCode::FORBIDDEN, "zugang.origin"); }
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return abgelehnt(&headers, s, m),
    };
    let guard = match state.rooms.join(&access.pad.id) {
        Ok(g) => g,
        Err(e) => { tracing::error!("Pad laden: {e}"); return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    };
    let st = state.clone();
    let author = access.author.as_ref().map(|(id, _)| id.clone());
    let readonly = access.readonly || author.is_none();
    upgrade.max_message_size(1 << 20).max_frame_size(1 << 20).on_upgrade(move |socket| sync::run(st, guard, socket, readonly, author))
}

/// Sicherheitskopfzeilen für Exporte und Stände (eigenständige Seiten ohne die CSP des Editors).
fn export_headers(resp: &mut Response) {
    let h = resp.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; script-src 'self'; font-src 'self'; base-uri 'none'; form-action 'none'"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
}

pub async fn export(st: State<Arc<AppState>>, p: Path<(String, String)>, headers: HeaderMap) -> Response {
    let mut r = export_inner(st, p, headers).await;
    export_headers(&mut r);
    r
}

async fn export_inner(State(state): State<Arc<AppState>>, Path((id, format)): Path<(String, String)>, headers: HeaderMap) -> Response {
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return fehler(&headers, s, m),
    };
    if let Some(addon) = crate::addons::von_pad(&state, &access.pad.id) {
        if !crate::addons::aktiv(&state, addon) { return crate::addons::nicht_aktiv(addon, &headers); }
        return (addon.export)(&state, &access.pad.id, &format, Sprache::aus_anfrage(&headers));
    }
    let sp = Sprache::aus_anfrage(&headers);
    let toc = texte::t(sp, "export.verzeichnis");
    match format.as_str() {
        "html" => match state.rooms.html_mit(&access.pad.id, toc).await {
            Ok(h) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], h).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        "txt" => match state.rooms.text_mit(&access.pad.id, toc).await {
            Ok(t) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], t).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        "md" => match state.rooms.markdown_mit(&access.pad.id, toc).await {
            Ok(t) => ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CONTENT_DISPOSITION, "inline; filename=\"pad.md\"")], t).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        "pdf" => match pad_pdf(&state, &access.pad.id, &id, sp).await {
            Ok(bytes) => {
                let name = format!("pad-{}.pdf", id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>());
                ([(header::CONTENT_TYPE, "application/pdf".to_string()), (header::CONTENT_DISPOSITION, format!("inline; filename=\"{name}\""))], bytes).into_response()
            }
            Err(e) => { tracing::warn!("PDF-Satz: {e}"); fehler(&headers, StatusCode::INTERNAL_SERVER_ERROR, "export.pdf_fehler") }
        },
        // Druckansicht für den Druckdialog des Browsers
        "print" => match state.rooms.html_mit(&access.pad.id, toc).await {
            Ok(h) => {
                let body = h.split("<body>").nth(1).and_then(|r| r.split("</body>").next()).unwrap_or("").to_string();
                let base = &state.config.base_path;
                let icons = icon_links();
                let page = format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<title>Pad</title>
<link rel="stylesheet" href="{base}/static/editor.css?v={v}"><style>body{{max-width:48em;margin:2em auto;padding:0 1em}} @media print{{ body{{margin:0;max-width:none}} .no-print{{display:none}} span.author{{background:transparent !important}} }}</style></head>
<body class="print-view"><p class="no-print">{hinweis} <a href="{base}/p/{id}">{zurueck}</a></p>
{body}<script src="{base}/static/print.js"></script></body></html>"#, v = bundle_version(), id = html_escape::encode_double_quoted_attribute(&id),
                    lang = sp.kuerzel(), hinweis = texte::t(sp, "export.druck_hinweis"), zurueck = texte::t(sp, "export.zum_pad"));
                ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page).into_response()
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn snapshots(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return abgelehnt(&headers, s, m),
    };
    match state.store.list_snapshots(&access.pad.id) {
        Ok(list) => axum::Json(json!(list.iter().map(|(ts, label)| json!({ "ts": ts, "label": label })).collect::<Vec<_>>())).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn snapshot_html(st: State<Arc<AppState>>, p: Path<(String, i64)>, headers: HeaderMap) -> Response {
    let mut r = snapshot_html_inner(st, p, headers).await;
    export_headers(&mut r);
    r
}

async fn snapshot_html_inner(State(state): State<Arc<AppState>>, Path((id, ts)): Path<(String, i64)>, headers: HeaderMap) -> Response {
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return abgelehnt(&headers, s, m),
    };
    match state.store.snapshot_html(&access.pad.id, ts) {
        Ok(Some(h)) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], h).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Frühere Stände zurückholen darf, wer das freie Pad besitzt; bei Moodle-Pads, wer moderiert (Lehrende).
fn may_restore(state: &AppState, access: &Access) -> bool {
    let Some((aid, _)) = access.author.as_ref().filter(|_| !access.readonly) else { return false };
    (access.free && state.store.owner_of(&access.pad.id).ok().flatten().as_deref() == Some(aid.as_str())) || state.store.is_moderator(&access.pad.id, aid)
}

/// Früheren Stand aus der Zeitleiste wiederherstellen: `{"ts": …}` für einen Vollstand, `{"seq": …}` für einen Schritt.
pub async fn restore(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, axum::Json(body): axum::Json<serde_json::Value>) -> Response {
    if !origin_ok(&state, &headers) || headers.get(header::ORIGIN).is_none() { return StatusCode::FORBIDDEN.into_response(); }
    let access = match check_access(&state, &headers, &id) { Ok(a) => a, Err((s, m)) => return abgelehnt(&headers, s, m) };
    if crate::addons::von_pad(&state, &access.pad.id).is_some() { return abgelehnt(&headers, StatusCode::CONFLICT, "stand.nur_text"); }
    if !may_restore(&state, &access) { return abgelehnt(&headers, StatusCode::FORBIDDEN, "stand.nur_besitz"); }
    let punkt = match (body.get("ts").and_then(|v| v.as_i64()), body.get("seq").and_then(|v| v.as_i64())) {
        (Some(ts), None) => crate::docs::Punkt::Stand(ts),
        (None, Some(seq)) => crate::docs::Punkt::Schritt(seq),
        _ => return abgelehnt(&headers, StatusCode::BAD_REQUEST, "stand.punkt_fehlt"),
    };
    match state.rooms.restore(&access.pad.id, punkt).await {
        Ok(Some(())) => { tracing::info!("Früheren Stand wiederhergestellt: Pad {}", access.pad.id); axum::Json(json!({ "ok": true })).into_response() }
        Ok(None) => abgelehnt(&headers, StatusCode::NOT_FOUND, "stand.fehlt"),
        Err(e) => { tracing::error!("Wiederherstellen: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Legt ein freies Pad an, höchstens `free_pads_per_hour` je Adresse.
fn create_free_pad(state: &AppState, name: &str, ip: std::net::IpAddr) -> Result<(), (StatusCode, &'static str)> {
    let key = crate::net::rate_key(ip);
    let now = ids::now_ms();
    {
        let mut m = state.free_creations.sperren();
        let list = m.entry(key).or_default();
        list.retain(|t| now - *t < 3_600_000);
        if list.len() as u32 >= state.config.free_pads_per_hour {
            return Err((StatusCode::TOO_MANY_REQUESTS, "frei.zu_viele"));
        }
        list.push(now);
    }
    match state.store.create_pad(name, None) {
        Ok(_) | Err(crate::store::CreatePadError::Exists) => Ok(()),
        Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "frei.anlegen")),
    }
}

fn redirect_to_pad(state: &AppState, name: &str) -> Response {
    let loc = format!("{}/p/{}", state.config.base_path, urlencoding(name));
    (StatusCode::SEE_OTHER, [(header::LOCATION, loc)]).into_response()
}

pub fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b { b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char), _ => out.push_str(&format!("%{b:02X}")) }
    }
    out
}

/// „Neues Pad“: zufälliger Name wie bei Etherpad.
pub async fn new_pad(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !state.config.allow_free_pads { return fehler(&headers, StatusCode::FORBIDDEN, "frei.aus"); }
    // „vorlage=ideen“ war bis 0.3.3 eine Textvorlage, alte Lesezeichen führen jetzt zur Ideensammlung
    let typ = q.get("typ").map(String::as_str).filter(|t| !t.is_empty()).or(q.get("vorlage").filter(|v| *v == "ideen").map(String::as_str));
    let addon = match typ.map(|t| crate::addons::nach_kennung(t).ok_or(t)) {
        None => None,
        Some(Ok(a)) if crate::addons::aktiv(&state, a) => Some(a),
        Some(Ok(a)) => return crate::addons::nicht_aktiv(a, &headers),
        Some(Err(_)) => return fehler(&headers, StatusCode::NOT_FOUND, "frei.typ_fehlt"),
    };
    let name = ids::random16();
    if let Err((s, m)) = create_free_pad(&state, &name, crate::net::client_ip(&state, &headers, peer)) { return fehler(&headers, s, m); }
    if let Some(a) = addon {
        if let Err(e) = state.store.set_kind(&name, a.art) { tracing::warn!("Pad-Typ: {e}"); }
    } else if let Some(html) = q.get("vorlage").and_then(|v| template(v, Sprache::aus_anfrage(&headers))) {
        if let Err(e) = state.rooms.set_html(&name, html).await { tracing::warn!("Vorlage: {e}"); }
    }
    let cookie = assign_owner(&state, &headers, &name);
    with_cookie(redirect_to_pad(&state, &name), cookie)
}

/// Wer ein freies Pad anlegt, besitzt es: angemeldete Person oder Gast (Gast-Cookie wird hier angelegt).
fn assign_owner(state: &AppState, headers: &HeaderMap, pad: &str) -> Option<String> {
    let (id, cookie) = match login_author(state, headers) {
        Some(a) => (a.id, None),
        None => match guest_author(state, headers, true) { Ok(Some((id, _, c))) => (id, c), _ => return None },
    };
    if let Err(e) = state.store.set_owner(pad, &id) { tracing::warn!("Besitz setzen: {e}"); }
    cookie
}

fn with_cookie(mut resp: Response, cookie: Option<String>) -> Response {
    if let Some(c) = cookie { if let Ok(v) = HeaderValue::from_str(&c) { resp.headers_mut().append(header::SET_COOKIE, v); } }
    resp
}

/// Vorlagen für neue Pads (Startseite: Gruppenarbeit, Protokoll), in der Sprache der Person.
fn template(name: &str, sp: Sprache) -> Option<&'static str> {
    Some(texte::t(sp, match name { "gruppenarbeit" => "vorlage.gruppenarbeit_html", "protokoll" => "vorlage.protokoll_html", _ => return None }))
}

/// „Pad öffnen“: Name aus dem Formular; legt an, wenn es das Pad noch nicht gibt.
pub async fn open_pad(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !state.config.allow_free_pads { return fehler(&headers, StatusCode::FORBIDDEN, "frei.aus"); }
    let name = q.get("padname").map(|s| s.trim().to_string()).unwrap_or_default();
    // ohne Namen wie „Neues Pad“: Zufallsname (Weiterleitung, damit Neuladen kein zweites Pad anlegt)
    if name.is_empty() { return (StatusCode::SEE_OTHER, [(header::LOCATION, format!("{}/new", state.config.base_path))]).into_response(); }
    if !valid_free_pad_name(&name) { return fehler(&headers, StatusCode::BAD_REQUEST, "frei.name"); }
    if state.store.get_pad(&name).ok().flatten().is_none() {
        if let Err((s, m)) = create_free_pad(&state, &name, crate::net::client_ip(&state, &headers, peer)) { return fehler(&headers, s, m); }
        let cookie = assign_owner(&state, &headers, &name);
        return with_cookie(redirect_to_pad(&state, &name), cookie);
    }
    redirect_to_pad(&state, &name)
}

/// Eigenen Namen setzen: nur Gäste (Token-Cookie). Namen aus Moodle setzt Moodle.
pub async fn rename_me(State(state): State<Arc<AppState>>, headers: HeaderMap, axum::Json(body): axum::Json<serde_json::Value>) -> Response {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().chars().filter(|c| !c.is_control()).take(60).collect::<String>();
    if name.is_empty() { return StatusCode::BAD_REQUEST.into_response(); }
    let author = cookie_values(&headers, &state.config.author_cookie).into_iter().find_map(|t| state.store.author_by_guest_token(&t).ok().flatten());
    let Some(a) = author else { return StatusCode::FORBIDDEN.into_response() };
    match state.store.rename_guest(&a.id, &name) { Ok(true) => axum::Json(json!({ "ok": true, "name": name })).into_response(), _ => StatusCode::FORBIDDEN.into_response() }
}

/// Startseite. Mit freien Pads: wie Etherpads Startseite (neues Pad, Pad per Name); sonst nur Hinweis.
pub async fn index(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let sp = Sprache::aus_anfrage(&headers);
    if state.config.allow_free_pads && theme_hsd(&state) {
        return start_page_hsd(&state, &headers);
    }
    if state.config.allow_free_pads {
        let base = &state.config.base_path;
        let icons = icon_links();
        let body = format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><title>NextPad</title>
<style>@font-face{{font-family:Quicksand;src:url("{base}/static/quicksand.ttf") format("truetype");font-weight:300 700}}
html,body{{height:100%;margin:0}} body{{background:url("{base}/static/fond.jpg") center center no-repeat fixed #64d29b;background-size:cover;font-family:Quicksand,Cantarell,"Open Sans","Helvetica Neue",sans-serif;font-size:16px;line-height:1.43;color:#333;display:flex;align-items:center;justify-content:center}}
#inner{{width:350px;max-width:92vw;text-align:center;color:#fff}}
#button{{display:block;width:100%;height:53px;line-height:53px;font:inherit;font-size:23px;color:#64d29b;background:#586a69;border:none;border-radius:3px;text-decoration:none;transition:.2s background}} #button:hover{{background:#4a5d5c;cursor:pointer}}
#label{{display:block;margin:18px 0 6px;font-weight:400}}
form.open{{display:flex;flex-wrap:wrap}} #padname{{flex:1 1 220px;height:38px;padding:0 12px;border:none;border-radius:3px 0 0 3px;font:inherit;font-size:14px;color:#4a5d5c;min-width:0}} form.open button{{height:38px;padding:0 14px;border:none;border-radius:0 3px 3px 0;background:#586a69;color:#64d29b;font:inherit;font-weight:700;cursor:pointer;transition:.2s background}} form.open button:hover{{background:#4a5d5c}}
.account{{margin-top:22px}} .account a.secondary,.account button{{display:inline-block;padding:9px 16px;border:2px solid #fff;border-radius:3px;color:#fff;background:transparent;font:inherit;text-decoration:none;cursor:pointer}} .account .me{{display:flex;align-items:center;gap:10px;justify-content:center}} .account img{{width:32px;height:32px;border-radius:50%}} .account form{{display:inline}} .account p{{margin:0 0 8px}}
p.hint{{font-size:13px;margin:26px 0 0;color:#fff;opacity:.9}} p.hint a{{color:#fff}} .sprachwahl a{{display:inline-block;min-width:44px;line-height:44px}} .sprachwahl a[aria-current]{{font-weight:700}}</style></head>
<body><div id="wrapper"><div id="inner">
<a id="button" href="{base}/new">{neu}</a>
<form class="open" action="{base}/open" method="get"><label id="label" for="padname" style="flex-basis:100%">{oder}</label>
<input type="text" id="padname" name="padname" maxlength="50" autofocus><button type="submit">{ok}</button></form>
{account}<p class="hint">NextPad{site}. {hinweis}</p>
<p class="hint">{wahl}</p>
</div></div></body></html>"#, account = account_block(&state, &headers, "colibris"), wahl = sprachwahl(&state, sp, "/"), site = site_suffix(&state), lang = sp.kuerzel(),
            neu = texte::t(sp, "start.c.neues_pad"), oder = texte::t(sp, "start.c.oder_name"), ok = texte::t(sp, "start.c.ok"), hinweis = texte::t(sp, "start.c.hinweis"));
        let mut resp = Html(body).into_response();
        resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return resp;
    }
    let base = &state.config.base_path;
    let (pads, _, _) = state.store.counts().unwrap_or((0, 0, 0));
    let icons = icon_links();
    let body = format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad</title>
<style>body{{font-family:Arial,Helvetica,sans-serif;color:#333;max-width:40em;margin:3em auto;padding:0 1em;line-height:1.5}} h1{{font-size:1.5em;border-bottom:2px solid #e60028;padding-bottom:.2em}} code{{background:#f3f3f3;padding:0 .3em}}</style></head>
<body><h1>NextPad</h1>
<p>{text}</p>
<p>{instanz}{betrieb}</p>
<p><a href="{base}/status">{status}</a> (JSON)</p>
<p>{rueckmeldung}</p>
<p>{wahl}</p></body></html>"#, lang = sp.kuerzel(), wahl = sprachwahl(&state, sp, "/"), text = texte::t(sp, "start.m.text_html"),
        instanz = texte::tf(sp, "start.m.instanz", &[("version", env!("CARGO_PKG_VERSION")), ("pads", &pads.to_string())]),
        status = texte::t(sp, "start.m.status"), rueckmeldung = texte::t(sp, "start.m.rueckmeldung"),
        betrieb = if state.config.site_name.is_empty() { String::new() } else { format!(" {}", html_escape::encode_text(&texte::tf(sp, "start.m.betrieb", &[("name", &state.config.site_name)]))) });
    let mut resp = Html(body).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Sprache wählen: `/sprache/en?zurueck=/p/…` merkt die Wahl ein Jahr und führt zurück. Geht ohne
/// Skript; SameSite=None, damit die Wahl auch in der in Moodle eingebetteten Aktivität gilt.
pub async fn sprache(State(state): State<Arc<AppState>>, Path(kuerzel): Path<String>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap) -> Response {
    let Some(sp) = Some(kuerzel.as_str()).filter(|k| k.len() == 2).and_then(Sprache::aus_kuerzel) else {
        return fehler(&headers, StatusCode::NOT_FOUND, "fehler.seite_fehlt");
    };
    let base = &state.config.base_path;
    let ziel = q.get("zurueck").filter(|z| eigener_pfad(base, z)).cloned().unwrap_or_else(|| format!("{base}/"));
    let path = if base.is_empty() { "/".to_string() } else { format!("{base}/") };
    let cookie = format!("{}={}; Path={path}; Max-Age=31536000; HttpOnly; Secure; SameSite=None", texte::WAHL_COOKIE, sp.kuerzel());
    // über Moodle angemeldet: Wahl auch für die Mail vor dem Löschen merken; nur, wenn der Klick von NextPad selbst
    // kommt (Sec-Fetch-Site), damit eine fremde Seite die gespeicherte Sprache nicht umstellen kann
    let eigen = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()).is_none_or(|v| v == "same-origin" || v == "none");
    if let Some(a) = login_author(&state, &headers).filter(|_| eigen) {
        if let Err(e) = state.store.set_author_sprache(&a.id, sp.kuerzel()) { tracing::warn!("Sprache nicht gespeichert: {e}"); }
    }
    with_cookie((StatusCode::SEE_OTHER, [(header::LOCATION, ziel), (header::CACHE_CONTROL, "no-store".to_string())]).into_response(), Some(cookie))
}

/// Nur Pfade dieses Servers als Rücksprungziel, keine fremden Adressen.
fn eigener_pfad(base: &str, z: &str) -> bool {
    z.starts_with('/') && !z.starts_with("//") && !z.contains('\\') && !z.chars().any(char::is_control)
        && (base.is_empty() || z == base || z.starts_with(&format!("{base}/")) || z.starts_with(&format!("{base}?")))
}

/// Unbekannte Pfade: kurze Seite statt leerer 404.
pub async fn not_found(headers: HeaderMap) -> Response {
    fehler(&headers, StatusCode::NOT_FOUND, "fehler.seite_fehlt")
}

/// Verlauf für die Zeitleiste: Snapshots (Vollstände) und Updates (Schritte) mit Zeit und Autor, Inhalte base64.
pub async fn history(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return abgelehnt(&headers, s, m),
    };
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let pad = &access.pad.id;
    if !state.store.flush() { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
    let (mut snaps, mut ups) = match (state.store.snapshots_all(pad), state.store.updates_all(pad)) {
        (Ok(s), Ok(u)) => (s, u),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    // Umfang begrenzen: die letzten 20 Vollstände, Schritte ab dem ältesten davon, höchstens 5000
    if snaps.len() > 20 { snaps.drain(..snaps.len() - 20); }
    if let Some((_, first_seq, _, _)) = snaps.first() { let s0 = *first_seq; ups.retain(|(seq, _, _, _)| *seq > s0); }
    let truncated = ups.len() > 5000;
    if truncated { ups.drain(..ups.len() - 5000); }
    let mut authors: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for (_, _, a, _) in &ups {
        if let Some(a) = a {
            if !authors.contains_key(a) {
                let name = state.store.get_author(a).ok().flatten().map(|x| x.name).unwrap_or_default();
                authors.insert(a.clone(), json!({ "name": name, "color": PALETTE[ids::color_index(a, PALETTE.len())] }));
            }
        }
    }
    let body = json!({
        "snapshots": snaps.iter().map(|(ts, seq, label, st)| json!({ "ts": ts, "seq": seq, "label": label, "state": b64.encode(st) })).collect::<Vec<_>>(),
        "updates": ups.iter().map(|(seq, ts, a, b)| json!({ "seq": seq, "ts": ts, "author": a, "bytes": b64.encode(b) })).collect::<Vec<_>>(),
        "authors": authors,
        "truncated": truncated,
    });
    let mut resp = axum::Json(body).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

pub async fn static_js() -> Response {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], EDITOR_JS).into_response()
}
fn theme_hsd(state: &AppState) -> bool { state.config.theme == "hsd" }
fn theme_file(state: &AppState, name: &str) -> Option<Vec<u8>> {
    let dir = state.config.theme_dir.as_ref()?;
    std::fs::read(dir.join(name)).ok()
}
/// Logo, Startbild und Hausschrift: Vorgabe aus NEXTPAD_THEME_DIR (nicht im Repo, Marke der Hochschule),
/// Logo und Startbild auch über die Verwaltungsseite (look.rs).
pub fn has_logo(state: &AppState) -> bool { crate::look::has_logo(state) }
fn has_background(state: &AppState) -> bool { crate::look::has_background(state) }
/// Adresse eines Theme-Bilds mit Stand, damit ein neues Bild sofort erscheint.
pub fn theme_url(state: &AppState, file: &str) -> String { format!("{}/theme/{file}?v={}", state.config.base_path, crate::look::get(state).version) }
/// Stilangaben aus dem Erscheinungsbild, nur im Design hsd.
pub fn look_css(state: &AppState) -> String { if theme_hsd(state) { crate::look::get(state).css() } else { String::new() } }
/// Admin: über Moodle angemeldet mit Admin-Recht.
pub fn login_admin(state: &AppState, headers: &HeaderMap) -> Option<crate::store::Author> {
    let now_s = ids::now_ms() / 1000;
    cookie_values(headers, &state.config.login_cookie).into_iter()
        .find(|t| state.store.login_is_admin(t, now_s).unwrap_or(false))
        .and_then(|t| state.store.author_by_login(&t, now_s).ok().flatten())
}
/// Einfache Seite im Rahmen der Startseite.
pub fn start_page(state: &AppState, sp: Sprache, wahl: Option<&str>, title: &str, content: &str, scripts: &str) -> Response {
    start_headers(state, Html(hsd_page(state, sp, wahl, title, "", "", content, scripts)).into_response())
}
pub async fn theme_font(State(state): State<Arc<AppState>>) -> Response {
    match theme_file(&state, "font.ttf") { Some(b) => ([(header::CONTENT_TYPE, "font/ttf"), (header::CACHE_CONTROL, "public, max-age=86400")], b).into_response(), None => StatusCode::NOT_FOUND.into_response() }
}
pub async fn static_theme_hsd() -> Response { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], THEME_HSD_CSS).into_response() }
pub async fn static_icons() -> Response { ([(header::CONTENT_TYPE, "font/woff"), (header::CACHE_CONTROL, "public, max-age=604800, immutable")], ICONS_WOFF).into_response() }
pub async fn static_fond() -> Response { ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "public, max-age=604800, immutable")], FOND_JPG).into_response() }
/// Zeichen von NextPad: Blase mit Text und Cursor, für Favicon und Kopfzeile.
pub fn icon_links() -> String {
    let base = BASE_PATH.get().map(String::as_str).unwrap_or("");
    format!(r#"<link rel="icon" href="{base}/static/logo.svg" type="image/svg+xml"><link rel="icon" href="{base}/static/favicon-32.png" sizes="32x32" type="image/png"><link rel="apple-touch-icon" href="{base}/static/apple-touch-icon.png">"#)
}
/// Das Zeichen inline, schmückend neben dem Schriftzug; nur wenn das Theme kein eigenes Logo mitbringt.
pub fn logo_mark(class: &str) -> String {
    LOGO_SVG.trim().replacen("<svg ", &format!(r#"<svg class="{class}" aria-hidden="true" focusable="false" "#), 1)
}
pub async fn static_logo() -> Response { ([(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "public, max-age=604800")], LOGO_SVG).into_response() }
pub async fn static_favicon() -> Response { ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=604800")], FAVICON_PNG).into_response() }
pub async fn static_touch_icon() -> Response { ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=604800")], TOUCH_ICON_PNG).into_response() }
pub async fn static_quicksand() -> Response { ([(header::CONTENT_TYPE, "font/ttf"), (header::CACHE_CONTROL, "public, max-age=604800, immutable")], QUICKSAND_TTF).into_response() }
pub async fn static_print() -> Response {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], "window.addEventListener('load', function () { setTimeout(function () { window.print() }, 200) })").into_response()
}
pub async fn static_css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], EDITOR_CSS).into_response()
}

pub async fn status(State(state): State<Arc<AppState>>) -> Response {
    let (pads, groups, authors) = state.store.counts().unwrap_or((0, 0, 0));
    axum::Json(json!({
        "name": "nextpad", "version": env!("CARGO_PKG_VERSION"), "apiVersion": state.config.api_version,
        "pads": pads, "groups": groups, "authors": authors, "loaded": state.rooms.loaded(),
    })).into_response()
}

pub const START_JS: &str = include_str!("../static/start.js");
pub async fn static_start() -> Response { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], START_JS).into_response() }

/// Anmeldebereich der Startseite: angemeldet (Name, Bild, Abmelden) oder Schaltflächen je Moodle.
fn account_block(state: &AppState, headers: &HeaderMap, theme: &str) -> String {
    let sp = Sprache::aus_anfrage(headers);
    let t = |k| texte::t(sp, k);
    let base = &state.config.base_path;
    if let Some(a) = login_author(state, headers) {
        let img = a.avatar.as_deref().map(|u| format!(r#"<img class="avatar" src="{}" alt="" width="40" height="40">"#, html_escape::encode_double_quoted_attribute(u))).unwrap_or_default();
        if theme == "hsd" {
            let admin = if login_admin(state, headers).is_some() { format!(r#"<a class="btn btn-secondary btn-block" href="{base}/admin/erscheinungsbild">{}</a>"#, t("konto.erscheinungsbild")) } else { String::new() };
            let wer = texte::tf(sp, "konto.angemeldet_als_html", &[("name", &html_escape::encode_text(&a.name))]);
            return format!(r#"<section class="account-line" aria-label="{anmeldung}"><div class="account-row">{img}<p class="who">{wer}</p>{admin}<form method="post" action="{base}/logout"><button type="submit" class="btn btn-secondary btn-block">{abmelden}</button></form></div></section>"#, anmeldung = t("konto.anmeldung"), abmelden = t("konto.abmelden"));
        }
        return format!(r#"<section class="panel account card" aria-labelledby="h-konto"><h2 id="h-konto">{angemeldet}</h2><div class="me">{img}<div class="who"><strong>{}</strong><span>{info}</span></div><form method="post" action="{base}/logout"><button type="submit" class="btn-link link">{abmelden}</button></form></div></section>"#, html_escape::encode_text(&a.name), angemeldet = t("konto.angemeldet"), info = t("konto.angemeldet_info"), abmelden = t("konto.abmelden"));
    }
    let buttons: String = state.config.lti.iter().filter_map(|p| p.start_url.as_ref().map(|u| (p, u)))
        .map(|(p, u)| format!(r#"<a class="btn btn-secondary btn-block secondary" href="{}">{}<span class="visually-hidden"> {}</span></a>"#, html_escape::encode_double_quoted_attribute(u), html_escape::encode_text(&p.label), t("konto.oeffnet_moodle"))).collect();
    if buttons.is_empty() { return String::new(); }
    if theme == "hsd" {
        return format!(r#"<section class="account-line" aria-label="{}"><div class="account-row">{buttons}<p class="hint">{}</p></div></section>"#, t("konto.anmeldung"), t("konto.hinweis"));
    }
    format!(r#"<section class="account card" aria-label="{}">{buttons}</section>"#, t("konto.anmeldung"))
}

/// Fußzeile: Hilfe, eingestellte Links (Datenschutz, Impressum …), Einrichtung.
fn footer_links(state: &AppState, sp: Sprache) -> String {
    let base = &state.config.base_path;
    let mut out = format!(r#"<a href="{base}/hilfe">{}</a>"#, texte::t(sp, "rahmen.hilfe"));
    for (l, u) in &state.config.footer_links {
        out.push_str(&format!(r#"<a href="{}">{}</a>"#, html_escape::encode_double_quoted_attribute(u), html_escape::encode_text(l)));
    }
    out
}

fn start_headers(state: &AppState, mut resp: Response) -> Response {
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: {}; font-src 'self'; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors 'self'; base-uri 'none'", moodle_origins(state));
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    resp
}

/// Betreiberangabe hinter „NextPad“ (NEXTPAD_SITE_NAME), leer ohne Angabe.
fn site_suffix(state: &AppState) -> String {
    let s = &state.config.site_name;
    if s.is_empty() { String::new() } else { format!(" · {}", html_escape::encode_text(s)) }
}

/// Umschalter „DE | EN“; `pfad` (ohne Basis-Pfad) ist die Seite, auf die die Wahl zurückführt.
fn sprachwahl(state: &AppState, sp: Sprache, pfad: &str) -> String {
    let base = &state.config.base_path;
    let zurueck = urlencoding(&format!("{base}{pfad}"));
    let link = |s: Sprache, kurz: &str, name: &str| {
        let aktuell = if s == sp { r#" aria-current="true""# } else { "" };
        format!(r#"<a href="{base}/sprache/{l}?zurueck={zurueck}" lang="{l}" hreflang="{l}" aria-label="{name}"{aktuell}>{kurz}</a>"#, l = s.kuerzel())
    };
    format!(r#"<span class="sprachwahl">{}<span aria-hidden="true">|</span>{}</span>"#, link(Sprache::De, "DE", "Deutsch"), link(Sprache::En, "EN", "English"))
}

/// Seitenrahmen im eingestellten Design; im Design colibris ohne Dachmarke. `wahl`: Pfad für den Sprachumschalter,
/// `None` auf Seiten, die es noch nicht in beiden Sprachen gibt.
pub fn page_frame(state: &AppState, sp: Sprache, wahl: Option<&str>, title: &str, head: &str, content: &str, scripts: &str) -> String {
    if theme_hsd(state) { return hsd_page_with(state, sp, wahl, title, head, "", "", content, scripts); }
    let base = &state.config.base_path;
    let icons = icon_links();
    let t = |k| texte::t(sp, k);
    format!(r##"<!doctype html><html lang="{lang}" data-base="{base}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover"><meta name="robots" content="noindex"><meta name="color-scheme" content="light dark"><title>{title}</title>
<link rel="stylesheet" href="{base}/static/start.css?v={v}">{head}</head>
<body class="theme-colibris"><a class="skip-link" href="#inhalt">{zum_inhalt}</a>
<header class="site-header"><a class="brand" href="{base}/">{mark}<strong>NextPad</strong></a><nav aria-label="{service}"><a href="{base}/hilfe">{hilfe}</a>{wahl}</nav></header>
{content}
<footer class="site-footer">{links}<span class="site">NextPad{site}</span></footer>
{scripts}</body></html>"##, v = start_version(), mark = logo_mark("brand-mark"), links = footer_links(state, sp), site = site_suffix(state),
        lang = sp.kuerzel(), zum_inhalt = t("rahmen.zum_inhalt"), service = t("rahmen.service"), hilfe = t("rahmen.hilfe"),
        wahl = wahl.map(|p| sprachwahl(state, sp, p)).unwrap_or_default())
}

/// Seitenrahmen im HSD-Design: Kopf mit Dachmarke und Produktname, Fuß mit Hilfe und Pflichtlinks.
#[allow(clippy::too_many_arguments)]
fn hsd_page(state: &AppState, sp: Sprache, wahl: Option<&str>, title: &str, body_class: &str, style_attr: &str, content: &str, scripts: &str) -> String {
    hsd_page_with(state, sp, wahl, title, "", body_class, style_attr, content, scripts)
}

#[allow(clippy::too_many_arguments)]
fn hsd_page_with(state: &AppState, sp: Sprache, wahl: Option<&str>, title: &str, head: &str, body_class: &str, style_attr: &str, content: &str, scripts: &str) -> String {
    let t = |k| texte::t(sp, k);
    let base = &state.config.base_path;
    let look = crate::look::get(state);
    let (product, tagline, inst) = (html_escape::encode_text(look.product()), html_escape::encode_text(look.tagline(sp)), html_escape::encode_double_quoted_attribute(look.institution()));
    // mit Logo der Einrichtung: Logo als Startseiten-Verweis, Produktname daneben; ohne: NextPad-Zeichen und Name als Verweis
    let brand = if has_logo(state) {
        format!(r#"<a class="brand" href="{base}/" aria-label="{label}"><img class="logo" src="{src}" alt="{inst}"></a><div class="product"><strong>{product}</strong><span>{tagline}</span></div>"#, src = theme_url(state, "logo.png"),
            label = html_escape::encode_double_quoted_attribute(&texte::tf(sp, "rahmen.startseite", &[("produkt", look.product()), ("einrichtung", look.institution())])))
    } else {
        format!(r#"<a class="product product-link" href="{base}/">{mark}<strong>{product}</strong><span>{tagline}</span></a>"#, mark = logo_mark("brand-mark"))
    };
    let links = footer_links(state, sp);
    let site = site_suffix(state);
    let icons = icon_links();
    format!(r##"<!doctype html><html lang="{lang}" data-base="{base}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover"><meta name="robots" content="noindex"><meta name="color-scheme" content="light dark"><title>{title}</title>
<link rel="stylesheet" href="{base}/static/start.css?v={v}"><style>@font-face{{font-family:"HSD Sans";src:url("{base}/theme/font.ttf") format("truetype");font-weight:400;font-display:swap}}</style>{css}{head}</head>
<body class="{body_class}"{style_attr}><a class="skip-link" href="#inhalt">{zum_inhalt}</a>
<header class="site-header">{brand}<nav aria-label="{service}"><a href="{base}/hilfe">{hilfe}</a>{wahl}</nav></header>
{content}
<footer class="site-footer">{links}<span class="site">NextPad{site}</span></footer>
{scripts}</body></html>"##, v = start_version(), css = look.css(), lang = sp.kuerzel(), zum_inhalt = t("rahmen.zum_inhalt"), service = t("rahmen.service"), hilfe = t("rahmen.hilfe"),
        wahl = wahl.map(|p| sprachwahl(state, sp, p)).unwrap_or_default())
}

/// Inhaltskennung von start.css/start.js für versionierte Adressen.
pub fn start_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for b in START_CSS.bytes().chain(START_JS.bytes()) { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        format!("{h:016x}")
    })
}

/// Startseite im HSD-Design: Fotoband mit
/// Pad-Namen als einziger Hauptsache, darunter auf ruhiger Fläche neu anlegen, zuletzt geöffnet, Anmeldung, Hinweis.
fn start_page_hsd(state: &AppState, headers: &HeaderMap) -> Response {
    let sp = Sprache::aus_anfrage(headers);
    let base = &state.config.base_path;
    let account = account_block(state, headers, "hsd");
    let bg = has_background(state);
    let credit = match (bg, crate::look::credit(state)) {
        (true, Some((c, url))) => {
            let c = html_escape::encode_text(&c);
            match &url { Some(u) => format!(r#"<p class="credit"><a href="{}">{c}</a></p>"#, html_escape::encode_double_quoted_attribute(u)), None => format!(r#"<p class="credit"><span>{c}</span></p>"#) }
        }
        _ => String::new(),
    };
    let style_attr = if bg { format!(r#" style="--bg-image:url('{}')""#, theme_url(state, "background.jpg")) } else { String::new() };
    let retention = retention_fact(state, sp);
    let addon_kacheln: String = crate::addons::aktive(state).map(|a| texte::t(sp, a.kachel).replace("{base}", base)).collect::<Vec<_>>().join("\n");
    let t = |k| texte::t(sp, k);
    let content = format!(r##"<div class="stage"><main id="inhalt" class="start" tabindex="-1">
<div class="hero">{credit}</div>
<div class="wrap entry-wrap">
<section class="panel entry" aria-labelledby="h-start">
<h1 id="h-start">{titel}</h1>
<p class="lead">{lead}</p>
<form class="join" action="{base}/open" method="get" novalidate>
<label class="field-label" for="padname">{name_label}</label>
<div class="field-row"><input type="text" id="padname" name="padname" maxlength="50" placeholder="{beispiel}" autocomplete="off" autocapitalize="none" spellcheck="false" enterkeyhint="go" aria-describedby="padname-hint padname-access padname-error"><button class="btn btn-primary" type="submit">{oeffnen}</button></div>
<p class="field-error" id="padname-error" role="alert" hidden></p>
<p class="hint" id="padname-hint">{name_hinweis}</p>
<p class="access" id="padname-access">{kein_passwort}</p>
</form>
<details class="new-pad" open>
<summary>{vorlagen}</summary>
<ul class="templates">
<li><a class="template" data-tint="blue" href="{base}/new?vorlage=gruppenarbeit"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M9 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM3 20c0-3.3 2.7-6 6-6s6 2.7 6 6M16 5.2a3 3 0 0 1 0 5.6M18 14.3c1.8.9 3 2.8 3 5.7"/></svg><span class="template-name">{gruppenarbeit}</span><span class="template-desc">{gruppenarbeit_info}</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
<li><a class="template" data-tint="green" href="{base}/new?vorlage=protokoll"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M6 3h12v18H6zM9 8h6M9 12h6M9 16h4"/></svg><span class="template-name">{protokoll}</span><span class="template-desc">{protokoll_info}</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
{addon_kacheln}
</ul>
</details>
</section>
</div>
<div class="wrap below">
<section class="recent" id="recent" hidden aria-labelledby="recent-h">
<h2 id="recent-h">{zuletzt}</h2>
<ul class="recent-list" id="recentlist"></ul>
<p class="hint">{zuletzt_hinweis} <button type="button" class="btn-link" id="recentclear">{leeren}</button></p>
</section>
{account}
<p class="note">{hinweis}{retention} <a href="{base}/hilfe">{mehr_hilfe}</a></p>
</div>
</main></div>"##, titel = t("start.titel"), lead = t("start.lead"), name_label = t("start.name_label"), beispiel = t("start.name_beispiel"),
        oeffnen = t("start.oeffnen"), name_hinweis = t("start.name_hinweis"), kein_passwort = t("start.kein_passwort"), vorlagen = t("start.vorlagen"),
        gruppenarbeit = t("start.gruppenarbeit"), gruppenarbeit_info = t("start.gruppenarbeit_info"), protokoll = t("start.protokoll"), protokoll_info = t("start.protokoll_info"),
        zuletzt = t("start.zuletzt"), zuletzt_hinweis = t("start.zuletzt_hinweis"), leeren = t("start.liste_leeren"), hinweis = t("start.hinweis"), mehr_hilfe = t("start.mehr_hilfe"));
    let scripts = format!(r#"<script type="application/json" id="start-texte">{json}</script><script src="{base}/static/start.js?v={v}"></script>"#, json = texte::fuer_skript(sp, "startjs."), v = start_version());
    let body_class = if bg { "has-bg" } else { "" };
    start_headers(state, Html(hsd_page(state, sp, Some("/"), "NextPad", body_class, &style_attr, &content, &scripts)).into_response())
}

/// Tatsächlicher Stand der Löschregel, nicht der geplante (keine Zusagen, die der Server nicht einlöst).
fn retention_fact(state: &AppState, sp: Sprache) -> String {
    match state.config.retention {
        crate::retention::Mode::On => format!(" {}", texte::tf(sp, "start.loeschfrist", &[("tage", &state.config.retention_days.to_string())])),
        _ => String::new(),
    }
}

/// Kurze Hilfe zur Startseite und zum Editor.
pub async fn help(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let sp = Sprache::aus_anfrage(&headers);
    let base = &state.config.base_path;
    let aufbewahrung = match state.config.retention {
        crate::retention::Mode::On => texte::tf(sp, "hilfe.aufbewahrung_an_html", &[("tage", &state.config.retention_days.to_string())]),
        _ => texte::t(sp, "hilfe.aufbewahrung_aus_html").to_string(),
    };
    let addon_hilfe: String = crate::addons::aktive(&state).map(|a| texte::t(sp, a.hilfe)).collect::<Vec<_>>().join("\n");
    let content = texte::tf(sp, "hilfe.seite_html", &[("base", base), ("addon_hilfe", &addon_hilfe), ("aufbewahrung", &aufbewahrung)]);
    let titel = format!("NextPad · {}", texte::t(sp, "hilfe.titel"));
    start_headers(&state, Html(hsd_page(&state, sp, Some("/hilfe"), &titel, "", "", &content, "")).into_response())
}

/// Abmelden per Adresse (GET): nicht sofort abmelden (Vorab-Laden durch Browser), sondern nachfragen.
pub async fn logout_confirm(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let sp = Sprache::aus_anfrage(&headers);
    let base = &state.config.base_path;
    let text = match login_author(&state, &headers) {
        Some(a) => format!(r#"<p>{}</p><form method="post" action="{base}/logout"><button type="submit" class="btn btn-primary">{abmelden}</button></form>"#, texte::tf(sp, "abmelden.angemeldet_html", &[("name", &html_escape::encode_text(&a.name))]), abmelden = texte::t(sp, "konto.abmelden")),
        None => format!("<p>{}</p>", texte::t(sp, "abmelden.niemand")),
    };
    let abmelden = texte::t(sp, "konto.abmelden");
    let content = format!(r#"<main id="inhalt" class="doc" tabindex="-1"><h1>{abmelden}</h1>{text}<p><a href="{base}/">{start}</a></p></main>"#, start = texte::t(sp, "abmelden.zur_startseite"));
    start_headers(&state, Html(hsd_page(&state, sp, Some("/logout"), &format!("NextPad · {abmelden}"), "", "", &content, "")).into_response())
}

/// Besitzer-Aktionen: nur per POST von der eigenen Seite, nur wer das freie Pad angelegt hat.
fn owner_check(state: &AppState, headers: &HeaderMap, id: &str) -> Result<String, Response> {
    if !origin_ok(state, headers) || headers.get(header::ORIGIN).is_none() { return Err(StatusCode::FORBIDDEN.into_response()); }
    let access = check_access(state, headers, id).map_err(|(s, m)| abgelehnt(headers, s, m))?;
    let aid = access.author.map(|a| a.0).unwrap_or_default();
    if !access.free || access.readonly || aid.is_empty() || state.store.owner_of(&access.pad.id).ok().flatten().as_deref() != Some(aid.as_str()) {
        return Err(abgelehnt(headers, StatusCode::FORBIDDEN, "zugang.nur_besitz"));
    }
    Ok(access.pad.id)
}

/// Besitzer verlängert die Aufbewahrung um ein Jahr.
pub async fn extend_owner(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let pad = match owner_check(&state, &headers, &id) { Ok(p) => p, Err(r) => return r };
    match crate::retention::extend(&state, &pad) {
        Ok(until) => axum::Json(json!({ "ok": true, "until": until })).into_response(),
        Err(e) => { tracing::warn!("Verlängern: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Besitzer löscht das Pad sofort.
pub async fn delete_owner(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let pad = match owner_check(&state, &headers, &id) { Ok(p) => p, Err(r) => return r };
    state.rooms.evict(&pad);
    match state.store.delete_pad(&pad) {
        Ok(_) => { tracing::info!("Pad durch Besitzer gelöscht"); axum::Json(json!({ "ok": true })).into_response() }
        Err(e) => { tracing::warn!("Löschen: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Link aus der Ankündigungsmail: erst bestätigen (Mailprogramme rufen Links teils selbst ab).
pub async fn extend_page(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap) -> Response {
    let base = &state.config.base_path;
    let t = q.get("t").cloned().unwrap_or_default();
    if t.len() != 48 || !t.chars().all(|c| c.is_ascii_alphanumeric()) || state.store.pad_by_extend_token(&t).ok().flatten().is_none() {
        return start_headers(&state, fehler(&headers, StatusCode::NOT_FOUND, "verlaengern.link_abgelaufen"));
    }
    let sp = Sprache::aus_anfrage(&headers);
    let icons = icon_links();
    let body = format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad · {titel}</title>
<style>body{{font-family:Arial,sans-serif;max-width:32em;margin:0 auto;padding:32px 16px;line-height:1.5}} button{{min-height:48px;padding:0 20px;background:#262626;color:#fff;border:0;font:inherit;font-weight:700;cursor:pointer}} h1{{font-size:24px;border-bottom:2px solid #e60028;padding-bottom:6px}}</style></head>
<body><h1>{titel}</h1><p>{text}</p><form method="post" action="{base}/verlaengern"><input type="hidden" name="t" value="{t}"><button type="submit">{knopf}</button></form></body></html>"#,
        lang = sp.kuerzel(), titel = texte::t(sp, "verlaengern.titel"), text = texte::t(sp, "verlaengern.text"), knopf = texte::t(sp, "verlaengern.knopf"));
    start_headers(&state, Html(body).into_response())
}

pub async fn extend_by_token(State(state): State<Arc<AppState>>, headers: HeaderMap, axum::Form(f): axum::Form<HashMap<String, String>>) -> Response {
    let t = f.get("t").cloned().unwrap_or_default();
    let pad = if t.len() == 48 && t.chars().all(|c| c.is_ascii_alphanumeric()) { state.store.pad_by_extend_token(&t).ok().flatten() } else { None };
    let Some(pad) = pad else { return start_headers(&state, fehler(&headers, StatusCode::NOT_FOUND, "verlaengern.link_ungueltig")) };
    match crate::retention::extend(&state, &pad) {
        Ok(until) => {
            let base = &state.config.base_path;
            let sp = Sprache::aus_anfrage(&headers);
            let icons = icon_links();
            let body = format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad · {fertig}</title><style>body{{font-family:Arial,sans-serif;max-width:32em;margin:0 auto;padding:32px 16px;line-height:1.5}}</style></head>
<body><h1>{fertig}</h1><p>{bis}</p><p><a href="{base}/p/{}">{zum_pad}</a></p></body></html>"#, urlencoding(&pad),
                lang = sp.kuerzel(), fertig = texte::t(sp, "verlaengern.fertig"), zum_pad = texte::t(sp, "verlaengern.zum_pad"),
                bis = texte::tf(sp, "verlaengern.bis", &[("datum", &crate::pdf::datum(sp, until))]));
            start_headers(&state, Html(body).into_response())
        }
        Err(_) => start_headers(&state, fehler(&headers, StatusCode::INTERNAL_SERVER_ERROR, "verlaengern.fehler")),
    }
}


/// PDF eines Pads im eingestellten Design (Typst), Rückfall auf das schlichte PDF.
pub async fn pad_pdf(state: &Arc<AppState>, pad_id: &str, shown_id: &str, sp: Sprache) -> anyhow::Result<Vec<u8>> {
    let body = state.rooms.typst_mit(pad_id, texte::t(sp, "export.verzeichnis")).await?;
    let md = state.rooms.markdown(pad_id).await?;
    let title = md.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or(shown_id).chars().take(80).collect::<String>();
    let title = html_escape::decode_html_entities(&title).to_string();
    let header = texte::tf(sp, "export.pdf_kopf", &[("datum", &crate::pdf::datum(sp, ids::now_ms()))]);
    let footer = format!("Pad {shown_id}");
    let st = state.clone();
    let res = tokio::task::spawn_blocking(move || {
        let look = crate::look::get(&st);
        let accent = look.palette().accent;
        let o = crate::typeset::Options { hsd: theme_hsd(&st), theme_dir: st.config.theme_dir.as_deref(), logo: crate::look::logo(&st), accent: &accent, product: look.product(), header: &header, footer: &footer, title: &title, sprache: sp };
        crate::typeset::pdf(&o, &body)
    }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    match res {
        Ok(b) => Ok(b),
        Err(e) => { tracing::warn!("Typst-Satz gescheitert, schlichtes PDF: {e}"); Ok(crate::pdf::from_markdown(&format!("NextPad · Pad {shown_id}"), &md, sp)) }
    }
}

pub const START_CSS: &str = include_str!("../static/start.css");
pub async fn static_start_css() -> Response { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], START_CSS).into_response() }

#[cfg(test)]
mod tests {
    use super::eigener_pfad;

    #[test]
    fn ruecksprung_nur_auf_eigene_pfade() {
        assert!(eigener_pfad("", "/p/abc?x=1"));
        assert!(eigener_pfad("/np", "/np/p/abc"));
        assert!(eigener_pfad("/np", "/np"));
        for z in ["https://evil.example/", "//evil.example/", "/\\evil.example", "p/abc", "/p/a\nb", ""] { assert!(!eigener_pfad("", z), "{z:?}"); }
        assert!(!eigener_pfad("/np", "/npx/p/abc"));
        assert!(!eigener_pfad("/np", "/p/abc"));
    }
}
