//! Editorseite `/p/<padID>`, WebSocket-Upgrade, Export, Statusseite.
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
fn moodle_origins(state: &AppState) -> String {
    let urls = state.config.api_key_moodle.iter().flatten().cloned().chain(state.config.lti.iter().map(|p| p.moodle()));
    urls.filter_map(|u| { let (scheme, rest) = u.split_once("://")?; Some(format!("{scheme}://{}", rest.split('/').next()?)) })
        .collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" ")
}

/// Gastautor aus dem eigenen Cookie (geheimes Token, nicht die öffentliche Autorenkennung).
/// Legt nur bei `create` einen neuen an (Editorseite); sonst ohne Cookie: kein Autor.
fn guest_author(state: &AppState, headers: &HeaderMap, create: bool) -> Result<Option<(String, String, Option<String>)>, (StatusCode, &'static str)> {
    let path = if state.config.base_path.is_empty() { "/".to_string() } else { format!("{}/", state.config.base_path) };
    let cookie = |token: &str| format!("{}={}; Path={}; Max-Age=31536000; HttpOnly; Secure; SameSite=Lax", state.config.author_cookie, token, path);
    for v in cookie_values(headers, &state.config.author_cookie) {
        if let Ok(Some(a)) = state.store.author_by_guest_token(&v) { return Ok(Some((a.id, a.name, None))); }
    }
    if !create { return Ok(None); }
    let a = state.store.create_author("Gast", None).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Autor konnte nicht angelegt werden."))?;
    let token = state.store.create_guest_token(&a.id).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Autor konnte nicht angelegt werden."))?;
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

/// Prüft Pad, Session und Lesemodus. Fehler: (Status, Meldung).
pub fn check_access(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Access, (StatusCode, &'static str)> { check_access_ext(state, headers, id, false) }

pub fn check_access_ext(state: &AppState, headers: &HeaderMap, id: &str, create_guest: bool) -> Result<Access, (StatusCode, &'static str)> {
    let (pad, readonly) = if id.starts_with("r.") {
        match state.store.pad_by_readonly(id) {
            Ok(Some(p)) => (p, true),
            _ => return Err((StatusCode::NOT_FOUND, "Dieses Pad gibt es nicht.")),
        }
    } else {
        match state.store.get_pad(id) {
            Ok(Some(p)) => (p, false),
            _ => return Err((StatusCode::NOT_FOUND, "Dieses Pad gibt es nicht.")),
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
        return Err((StatusCode::FORBIDDEN, "Pads ohne Gruppe sind auf diesem Server nicht erreichbar."));
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
    Err((StatusCode::FORBIDDEN, "Keine gültige Sitzung für dieses Pad. Bitte die Seite in Moodle neu laden."))
}

/// Basis-Pfad für Seiten ohne Zugriff auf den Zustand (Fehlerseiten); einmal beim Start gesetzt.
pub static BASE_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn error_page(status: StatusCode, msg: &str) -> Response {
    let base = BASE_PATH.get().map(String::as_str).unwrap_or("");
    let title = match status.as_u16() { 404 => "Nicht gefunden", 403 => "Kein Zugriff", 400 => "Eingabe prüfen", 429 => "Bitte kurz warten", _ => "Das hat nicht geklappt" };
    let icons = icon_links();
    (status, Html(format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light dark"><title>{title} · NextPad</title><link rel="stylesheet" href="{base}/static/start.css?v={v}"></head>
<body><main class="doc"><h1>{title}</h1><p class="notice" role="alert">{msg}</p><p><a class="btn btn-secondary" href="{base}/">Zur Startseite von NextPad</a></p></main></body></html>"#, msg = html_escape::encode_text(msg), v = start_version()))).into_response()

}

pub async fn page(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match check_access_ext(&state, &headers, &id, true) {
        Ok(a) => a,
        Err((s, m)) => return error_page(s, m),
    };
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
        "expires": expires,
        "theme": state.config.theme,
        "logo": if theme_hsd(&state) && has_logo(&state) { serde_json::Value::String(theme_url(&state, "logo.png")) } else { serde_json::Value::Null },
        "course": if access.free { None } else { state.store.course_url(&access.pad.id) },
    });
    let base = &state.config.base_path;
    if state.store.is_ideas(&access.pad.id) {
        let mut cfg = cfg;
        cfg["readonlyId"] = if access.readonly { serde_json::Value::Null } else { json!(access.pad.readonly_id) };
        let resp = crate::ideas::page(&state, &id, cfg);
        return with_cookie(resp, access.set_cookie.clone());
    }
    if state.store.is_quiz(&access.pad.id) {
        let join = crate::quiz::join_svg(&state, &headers, access.free, &id, &access.pad.id, "");
        let resp = crate::quiz::page(&state, &id, cfg, &join);
        return with_cookie(resp, access.set_cookie.clone());
    }
    if state.store.is_board(&access.pad.id) {
        let icons = icon_links();
        let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad · Zeichenbrett</title>
<link rel="stylesheet" href="{base}/static/board/board.css?v={v}"><style>html,body{{margin:0;height:100%;overflow:hidden}} #board{{position:fixed;inset:0}}</style></head>
<body><div id="board"></div>
<script type="application/json" id="nextpad-config">{cfg}</script>
<script src="{base}/static/board/assets-path.js?v={v}"></script>
<script type="module" src="{base}/static/board/board.js?v={v}"></script></body></html>"#, v = board_version(), cfg = cfg.to_string().replace("</", "<\\/"));
        let mut resp = Html(body).into_response();
        let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: {}; font-src 'self' data:; worker-src 'self' blob:; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors {}; base-uri 'none'", moodle_origins(&state), state.config.frame_ancestors);
        let h = resp.headers_mut();
        if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
        h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(c) = &access.set_cookie { if let Ok(v) = HeaderValue::from_str(c) { h.append(header::SET_COOKIE, v); } }
        return resp;
    }
    let theme_link = if theme_hsd(&state) { format!(r#"<link rel="stylesheet" href="{}/static/theme-hsd.css?v={}">"#, state.config.base_path, bundle_version()) } else { String::new() };
    let body_class = if theme_hsd(&state) { "theme-hsd" } else { "theme-colibris" };
    let icons = icon_links();
    let body = format!(
        r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad</title>
<link rel="stylesheet" href="{base}/static/editor.css?v={v}">{theme_link}{look}</head>
<body class="{body_class}"><div id="app"><div id="toolbar" role="toolbar" aria-label="Formatierung"></div><div id="editorcontainerbox"><div id="editor"></div></div><div id="statusbar"></div></div>
<script type="application/json" id="nextpad-config">{cfg}</script>
<script src="{base}/static/editor.js?v={v}"></script></body></html>"#,
        v = bundle_version(),
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
    if !origin_ok(&state, &headers) { return (StatusCode::FORBIDDEN, "Origin nicht erlaubt").into_response(); }
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return (s, m).into_response(),
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
        Err((s, m)) => return error_page(s, m),
    };
    if state.store.is_quiz(&access.pad.id) {
        return crate::quiz::export(&state, &access.pad.id, &format).unwrap_or_else(|| error_page(StatusCode::NOT_FOUND, "Quiz-Raster lassen sich als Markdown oder Text sichern (nur der Punktestand)."));
    }
    if state.store.is_ideas(&access.pad.id) {
        return crate::ideas::export(&state, &access.pad.id, &format).unwrap_or_else(|| error_page(StatusCode::NOT_FOUND, "Ideensammlungen lassen sich als Markdown, Text oder CSV sichern."));
    }
    if state.store.is_board(&access.pad.id) {
        return error_page(StatusCode::CONFLICT, "Zeichenbretter lassen sich im Zeichenbrett über das Menü als PNG, SVG oder Excalidraw-Datei sichern.");
    }
    match format.as_str() {
        "html" => match state.rooms.html(&access.pad.id).await {
            Ok(h) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], h).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        "txt" => match state.rooms.text(&access.pad.id).await {
            Ok(t) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], t).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        "md" => match state.rooms.markdown(&access.pad.id).await {
            Ok(t) => ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CONTENT_DISPOSITION, "inline; filename=\"pad.md\"")], t).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        "pdf" => match pad_pdf(&state, &access.pad.id, &id).await {
            Ok(bytes) => {
                let name = format!("pad-{}.pdf", id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>());
                ([(header::CONTENT_TYPE, "application/pdf".to_string()), (header::CONTENT_DISPOSITION, format!("inline; filename=\"{name}\""))], bytes).into_response()
            }
            Err(e) => { tracing::warn!("PDF-Satz: {e}"); error_page(StatusCode::INTERNAL_SERVER_ERROR, "Das PDF konnte nicht erzeugt werden. Die Druckansicht geht weiter über Export, Drucken.") }
        },
        // Druckansicht für den Druckdialog des Browsers
        "print" => match state.rooms.html(&access.pad.id).await {
            Ok(h) => {
                let body = h.split("<body>").nth(1).and_then(|r| r.split("</body>").next()).unwrap_or("").to_string();
                let base = &state.config.base_path;
                let icons = icon_links();
                let page = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<title>Pad</title>
<link rel="stylesheet" href="{base}/static/editor.css?v={v}"><style>body{{max-width:48em;margin:2em auto;padding:0 1em}} @media print{{ body{{margin:0;max-width:none}} .no-print{{display:none}} span.author{{background:transparent !important}} }}</style></head>
<body class="print-view"><p class="no-print">Druckansicht. Im Druckdialog „Als PDF speichern“ wählen. <a href="{base}/p/{id}">Zurück zum Pad</a></p>
{body}<script src="{base}/static/print.js"></script></body></html>"#, v = bundle_version(), id = html_escape::encode_double_quoted_attribute(&id));
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
        Err((s, m)) => return (s, m).into_response(),
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
        Err((s, m)) => return (s, m).into_response(),
    };
    match state.store.snapshot_html(&access.pad.id, ts) {
        Ok(Some(h)) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], h).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Legt ein freies Pad an, höchstens `free_pads_per_hour` je Adresse.
fn create_free_pad(state: &AppState, name: &str, ip: std::net::IpAddr) -> Result<(), (StatusCode, &'static str)> {
    let key = crate::net::rate_key(ip);
    let now = ids::now_ms();
    {
        let mut m = state.free_creations.lock().unwrap_or_else(|e| e.into_inner());
        let list = m.entry(key).or_default();
        list.retain(|t| now - *t < 3_600_000);
        if list.len() as u32 >= state.config.free_pads_per_hour {
            return Err((StatusCode::TOO_MANY_REQUESTS, "Zu viele neue Pads in kurzer Zeit. Bitte später noch einmal."));
        }
        list.push(now);
    }
    match state.store.create_pad(name, None) {
        Ok(_) | Err(crate::store::CreatePadError::Exists) => Ok(()),
        Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "Pad konnte nicht angelegt werden.")),
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
    if !state.config.allow_free_pads { return error_page(StatusCode::FORBIDDEN, "Freie Pads sind auf diesem Server nicht eingeschaltet."); }
    let name = ids::random16();
    if let Err((s, m)) = create_free_pad(&state, &name, crate::net::client_ip(&state, &headers, peer)) { return error_page(s, m); }
    // „vorlage=ideen“ war bis 0.3.3 eine Textvorlage, alte Lesezeichen führen jetzt zur Ideensammlung
    let kind = match (q.get("typ").map(String::as_str), q.get("vorlage").map(String::as_str)) {
        (Some("zeichenbrett"), _) => Some("board"),
        (Some("ideen"), _) | (_, Some("ideen")) => Some("ideas"),
        (Some("quiz"), _) => Some("quiz"),
        _ => None,
    };
    if let Some(k) = kind {
        if let Err(e) = state.store.set_kind(&name, k) { tracing::warn!("Pad-Typ: {e}"); }
    } else if let Some(html) = q.get("vorlage").and_then(|v| template(v)) {
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

/// Vorlagen für neue Pads (Startseite: Gruppenarbeit, Protokoll).
fn template(name: &str) -> Option<&'static str> {
    Some(match name {
        "gruppenarbeit" => "<h1>Gruppenarbeit</h1><h2>Aufgabe</h2><p>Worum geht es, was soll am Ende stehen?</p><h2>Wer macht was?</h2><ul><li>Name: Teilaufgabe</li><li>Name: Teilaufgabe</li></ul><h2>Ergebnisse</h2><p></p><h2>Offene Fragen</h2><ul><li></li></ul>",
        "protokoll" => "<h1>Protokoll</h1><p>Datum: </p><p>Anwesend: </p><p>Protokoll: </p><h2>Tagesordnung</h2><ol><li></li><li></li></ol><h2>Ergebnisse und Beschlüsse</h2><ul><li></li></ul><h2>Aufgaben</h2><ul><li>Was, wer, bis wann</li></ul>",
        _ => return None,
    })
}

/// „Pad öffnen“: Name aus dem Formular; legt an, wenn es das Pad noch nicht gibt.
pub async fn open_pad(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !state.config.allow_free_pads { return error_page(StatusCode::FORBIDDEN, "Freie Pads sind auf diesem Server nicht eingeschaltet."); }
    let name = q.get("padname").map(|s| s.trim().to_string()).unwrap_or_default();
    // ohne Namen wie „Neues Pad“: Zufallsname (Weiterleitung, damit Neuladen kein zweites Pad anlegt)
    if name.is_empty() { return (StatusCode::SEE_OTHER, [(header::LOCATION, format!("{}/new", state.config.base_path))]).into_response(); }
    if !valid_free_pad_name(&name) { return error_page(StatusCode::BAD_REQUEST, "Pad-Name: 1 bis 50 Zeichen aus Buchstaben, Ziffern, Leerzeichen und - _ . ( ) ,"); }
    if state.store.get_pad(&name).ok().flatten().is_none() {
        if let Err((s, m)) = create_free_pad(&state, &name, crate::net::client_ip(&state, &headers, peer)) { return error_page(s, m); }
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
    if state.config.allow_free_pads && theme_hsd(&state) {
        return start_page_hsd(&state, &headers);
    }
    if state.config.allow_free_pads {
        let base = &state.config.base_path;
        let icons = icon_links();
        let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><title>NextPad</title>
<style>@font-face{{font-family:Quicksand;src:url("{base}/static/quicksand.ttf") format("truetype");font-weight:300 700}}
html,body{{height:100%;margin:0}} body{{background:url("{base}/static/fond.jpg") center center no-repeat fixed #64d29b;background-size:cover;font-family:Quicksand,Cantarell,"Open Sans","Helvetica Neue",sans-serif;font-size:16px;line-height:1.43;color:#333;display:flex;align-items:center;justify-content:center}}
#inner{{width:350px;max-width:92vw;text-align:center;color:#fff}}
#button{{display:block;width:100%;height:53px;line-height:53px;font:inherit;font-size:23px;color:#64d29b;background:#586a69;border:none;border-radius:3px;text-decoration:none;transition:.2s background}} #button:hover{{background:#4a5d5c;cursor:pointer}}
#label{{display:block;margin:18px 0 6px;font-weight:400}}
form.open{{display:flex;flex-wrap:wrap}} #padname{{flex:1 1 220px;height:38px;padding:0 12px;border:none;border-radius:3px 0 0 3px;font:inherit;font-size:14px;color:#4a5d5c;min-width:0}} form.open button{{height:38px;padding:0 14px;border:none;border-radius:0 3px 3px 0;background:#586a69;color:#64d29b;font:inherit;font-weight:700;cursor:pointer;transition:.2s background}} form.open button:hover{{background:#4a5d5c}}
.account{{margin-top:22px}} .account a.secondary,.account button{{display:inline-block;padding:9px 16px;border:2px solid #fff;border-radius:3px;color:#fff;background:transparent;font:inherit;text-decoration:none;cursor:pointer}} .account .me{{display:flex;align-items:center;gap:10px;justify-content:center}} .account img{{width:32px;height:32px;border-radius:50%}} .account form{{display:inline}} .account p{{margin:0 0 8px}}
p.hint{{font-size:13px;margin:26px 0 0;color:#fff;opacity:.9}} p.hint a{{color:#fff}}</style></head>
<body><div id="wrapper"><div id="inner">
<a id="button" href="{base}/new">Neues Pad</a>
<form class="open" action="{base}/open" method="get"><label id="label" for="padname" style="flex-basis:100%">oder ein Pad mit folgendem Namen erstellen/öffnen:</label>
<input type="text" id="padname" name="padname" maxlength="50" autofocus><button type="submit">OK</button></form>
{account}<p class="hint">NextPad{site}. Wer den Namen eines Pads kennt, kann es lesen und schreiben. Kurse nutzen die Aktivität „Etherpad Lite“ in Moodle.</p>
</div></div></body></html>"#, account = account_block(&state, &headers, "colibris"), site = site_suffix(&state));
        let mut resp = Html(body).into_response();
        resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return resp;
    }
    let base = &state.config.base_path;
    let (pads, _, _) = state.store.counts().unwrap_or((0, 0, 0));
    let icons = icon_links();
    let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad</title>
<style>body{{font-family:Arial,Helvetica,sans-serif;color:#333;max-width:40em;margin:3em auto;padding:0 1em;line-height:1.5}} h1{{font-size:1.5em;border-bottom:2px solid #e60028;padding-bottom:.2em}} code{{background:#f3f3f3;padding:0 .3em}}</style></head>
<body><h1>NextPad</h1>
<p>NextPad: ein Pad-Server, der die Etherpad-Schnittstelle für das Moodle-Plugin <em>Etherpad Lite</em> nachbildet. Pads werden aus Moodle-Kursen heraus geöffnet; einen direkten Einstieg gibt es hier nicht.</p>
<p>Diese Instanz: Version {version}, {pads} Pads.{betrieb}</p>
<p><a href="{base}/status">Statusseite</a> (JSON)</p>
<p>Wer ein Pad sieht und etwas fehlt: Rückmeldung an den Betrieb dieser Instanz.</p></body></html>"#, version = env!("CARGO_PKG_VERSION"),
        betrieb = if state.config.site_name.is_empty() { String::new() } else { format!(" Betrieb: {}.", html_escape::encode_text(&state.config.site_name)) });
    let mut resp = Html(body).into_response();
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Unbekannte Pfade: kurze Seite statt leerer 404.
pub async fn not_found() -> Response {
    error_page(StatusCode::NOT_FOUND, "Diese Seite gibt es nicht. Pads werden aus Moodle heraus geöffnet.")
}

/// Verlauf für die Zeitleiste: Snapshots (Vollstände) und Updates (Schritte) mit Zeit und Autor, Inhalte base64.
pub async fn history(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match check_access(&state, &headers, &id) {
        Ok(a) => a,
        Err((s, m)) => return (s, m).into_response(),
    };
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let pad = &access.pad.id;
    state.store.flush();
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
pub fn start_page(state: &AppState, title: &str, content: &str, scripts: &str) -> Response {
    start_headers(state, Html(hsd_page(state, title, "", "", content, scripts)).into_response())
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
    let base = &state.config.base_path;
    if let Some(a) = login_author(state, headers) {
        let img = a.avatar.as_deref().map(|u| format!(r#"<img class="avatar" src="{}" alt="" width="40" height="40">"#, html_escape::encode_double_quoted_attribute(u))).unwrap_or_default();
        if theme == "hsd" {
            let admin = if login_admin(state, headers).is_some() { format!(r#"<a class="btn btn-secondary btn-block" href="{base}/admin/erscheinungsbild">Erscheinungsbild</a>"#) } else { String::new() };
            return format!(r#"<section class="account-line" aria-label="Anmeldung"><div class="account-row">{img}<p class="who">Über Moodle angemeldet als <strong>{}</strong></p>{admin}<form method="post" action="{base}/logout"><button type="submit" class="btn btn-secondary btn-block">Abmelden</button></form></div></section>"#, html_escape::encode_text(&a.name));
        }
        return format!(r#"<section class="panel account card" aria-labelledby="h-konto"><h2 id="h-konto">Angemeldet</h2><div class="me">{img}<div class="who"><strong>{}</strong><span>über Moodle angemeldet, Name und Bild erscheinen in Pads</span></div><form method="post" action="{base}/logout"><button type="submit" class="btn-link link">Abmelden</button></form></div></section>"#, html_escape::encode_text(&a.name));
    }
    let buttons: String = state.config.lti.iter().filter_map(|p| p.start_url.as_ref().map(|u| (p, u)))
        .map(|(p, u)| format!(r#"<a class="btn btn-secondary btn-block secondary" href="{}">{}<span class="visually-hidden"> (öffnet Moodle)</span></a>"#, html_escape::encode_double_quoted_attribute(u), html_escape::encode_text(&p.label))).collect();
    if buttons.is_empty() { return String::new(); }
    if theme == "hsd" {
        return format!(r#"<section class="account-line" aria-label="Anmeldung"><div class="account-row">{buttons}<p class="hint">Name und Profilbild aus Moodle erscheinen dann im Pad. Es geht auch ohne.</p></div></section>"#);
    }
    format!(r#"<section class="account card" aria-label="Anmeldung">{buttons}</section>"#)
}

/// Fußzeile: Hilfe, eingestellte Links (Datenschutz, Impressum …), Einrichtung.
fn footer_links(state: &AppState) -> String {
    let base = &state.config.base_path;
    let mut out = format!(r#"<a href="{base}/hilfe">Hilfe</a>"#);
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

/// Seitenrahmen im eingestellten Design; im Design colibris ohne Dachmarke.
pub fn page_frame(state: &AppState, title: &str, head: &str, content: &str, scripts: &str) -> String {
    if theme_hsd(state) { return hsd_page_with(state, title, head, "", "", content, scripts); }
    let base = &state.config.base_path;
    let icons = icon_links();
    format!(r##"<!doctype html><html lang="de" data-base="{base}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover"><meta name="robots" content="noindex"><meta name="color-scheme" content="light dark"><title>{title}</title>
<link rel="stylesheet" href="{base}/static/start.css?v={v}">{head}</head>
<body class="theme-colibris"><a class="skip-link" href="#inhalt">Zum Inhalt springen</a>
<header class="site-header"><a class="brand" href="{base}/">{mark}<strong>NextPad</strong></a><nav aria-label="Service"><a href="{base}/hilfe">Hilfe</a></nav></header>
{content}
<footer class="site-footer">{links}<span class="site">NextPad{site}</span></footer>
{scripts}</body></html>"##, v = start_version(), mark = logo_mark("brand-mark"), links = footer_links(state), site = site_suffix(state))
}

/// Seitenrahmen im HSD-Design: Kopf mit Dachmarke und Produktname, Fuß mit Hilfe und Pflichtlinks.
fn hsd_page(state: &AppState, title: &str, body_class: &str, style_attr: &str, content: &str, scripts: &str) -> String {
    hsd_page_with(state, title, "", body_class, style_attr, content, scripts)
}

fn hsd_page_with(state: &AppState, title: &str, head: &str, body_class: &str, style_attr: &str, content: &str, scripts: &str) -> String {
    let base = &state.config.base_path;
    let look = crate::look::get(state);
    let (product, tagline, inst) = (html_escape::encode_text(look.product()), html_escape::encode_text(look.tagline()), html_escape::encode_double_quoted_attribute(look.institution()));
    // mit Logo der Einrichtung: Logo als Startseiten-Verweis, Produktname daneben; ohne: NextPad-Zeichen und Name als Verweis
    let brand = if has_logo(state) {
        format!(r#"<a class="brand" href="{base}/" aria-label="{product} Startseite, {inst}"><img class="logo" src="{src}" alt="{inst}"></a><div class="product"><strong>{product}</strong><span>{tagline}</span></div>"#, src = theme_url(state, "logo.png"))
    } else {
        format!(r#"<a class="product product-link" href="{base}/">{mark}<strong>{product}</strong><span>{tagline}</span></a>"#, mark = logo_mark("brand-mark"))
    };
    let links = footer_links(state);
    let site = site_suffix(state);
    let icons = icon_links();
    format!(r##"<!doctype html><html lang="de" data-base="{base}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover"><meta name="robots" content="noindex"><meta name="color-scheme" content="light dark"><title>{title}</title>
<link rel="stylesheet" href="{base}/static/start.css?v={v}"><style>@font-face{{font-family:"HSD Sans";src:url("{base}/theme/font.ttf") format("truetype");font-weight:400;font-display:swap}}</style>{css}{head}</head>
<body class="{body_class}"{style_attr}><a class="skip-link" href="#inhalt">Zum Inhalt springen</a>
<header class="site-header">{brand}<nav aria-label="Service"><a href="{base}/hilfe">Hilfe</a></nav></header>
{content}
<footer class="site-footer">{links}<span class="site">NextPad{site}</span></footer>
{scripts}</body></html>"##, v = start_version(), css = look.css())
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
    let retention = retention_fact(state);
    let content = format!(r##"<div class="stage"><main id="inhalt" class="start" tabindex="-1">
<div class="hero">{credit}</div>
<div class="wrap entry-wrap">
<section class="panel entry" aria-labelledby="h-start">
<h1 id="h-start">Gemeinsam schreiben</h1>
<p class="lead">Der gemeinsame Schreibraum für Studium, Lehre und Zusammenarbeit. Im Browser, ohne Konto.</p>
<form class="join" action="{base}/open" method="get" novalidate>
<label class="field-label" for="padname">Name des Pads</label>
<div class="field-row"><input type="text" id="padname" name="padname" maxlength="50" placeholder="z. B. gruppe-3" autocomplete="off" autocapitalize="none" spellcheck="false" enterkeyhint="go" aria-describedby="padname-hint padname-access padname-error"><button class="btn btn-primary" type="submit">Öffnen oder anlegen</button></div>
<p class="field-error" id="padname-error" role="alert" hidden></p>
<p class="hint" id="padname-hint">Gibt es das Pad schon, öffnet es sich. Sonst wird es neu angelegt. Ohne Namen entsteht ein Pad mit Zufallsnamen.</p>
<p class="access" id="padname-access">Kein Passwortschutz: Wer den Namen oder Link kennt, kann mitlesen und mitschreiben.</p>
</form>
<details class="new-pad" open>
<summary>Mit Vorlage beginnen</summary>
<ul class="templates">
<li><a class="template" data-tint="blue" href="{base}/new?vorlage=gruppenarbeit"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M9 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM3 20c0-3.3 2.7-6 6-6s6 2.7 6 6M16 5.2a3 3 0 0 1 0 5.6M18 14.3c1.8.9 3 2.8 3 5.7"/></svg><span class="template-name">Gruppenarbeit</span><span class="template-desc">Aufgabe, wer macht was, Ergebnisse</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
<li><a class="template" data-tint="green" href="{base}/new?vorlage=protokoll"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M6 3h12v18H6zM9 8h6M9 12h6M9 16h4"/></svg><span class="template-name">Protokoll</span><span class="template-desc">Tagesordnung, Beschlüsse, Aufgaben</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
<li><a class="template" data-tint="yellow" href="{base}/new?typ=ideen"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3z"/></svg><span class="template-name">Ideensammlung</span><span class="template-desc">Ideen und Fragen sammeln, abstimmen</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
<li><a class="template" data-tint="blue" href="{base}/new?typ=quiz"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z"/></svg><span class="template-name">Quiz-Raster</span><span class="template-desc">Fragen nach Punkten, Teams, Beamer</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
<li><a class="template" href="{base}/new?typ=zeichenbrett"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3M4 20h16"/></svg><span class="template-name">Zeichenbrett</span><span class="template-desc">Gemeinsam skizzieren</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>
</ul>
</details>
</section>
</div>
<div class="wrap below">
<section class="recent" id="recent" hidden aria-labelledby="recent-h">
<h2 id="recent-h">Zuletzt geöffnet</h2>
<ul class="recent-list" id="recentlist"></ul>
<p class="hint">Nur in diesem Browser gespeichert. <button type="button" class="btn-link" id="recentclear">Liste leeren</button></p>
</section>
{account}
<p class="note">Bitte keine Daten anderer Personen eintragen. Für Kurse mit festen Gruppen eignet sich die Pad-Aktivität in Moodle.{retention} <a href="{base}/hilfe">Mehr in der Hilfe</a></p>
</div>
</main></div>"##);
    let scripts = format!(r#"<script src="{base}/static/start.js?v={v}"></script>"#, v = start_version());
    let body_class = if bg { "has-bg" } else { "" };
    start_headers(state, Html(hsd_page(state, "NextPad", body_class, &style_attr, &content, &scripts)).into_response())
}

/// Tatsächlicher Stand der Löschregel, nicht der geplante (keine Zusagen, die der Server nicht einlöst).
fn retention_fact(state: &AppState) -> String {
    match state.config.retention {
        crate::retention::Mode::On => format!(" Pads, die {} Tage weder geöffnet noch bearbeitet werden, löscht NextPad.", state.config.retention_days),
        _ => String::new(),
    }
}

/// Kurze Hilfe zur Startseite und zum Editor.
pub async fn help(State(state): State<Arc<AppState>>) -> Response {
    let base = &state.config.base_path;
    let retention = match state.config.retention {
        crate::retention::Mode::On => format!("<p>Pads aus Moodle bleiben, solange es die Aktivität im Kurs gibt. Pads von der Startseite gehören der Person, die sie angelegt hat. Nach {} Tagen ohne Öffnen oder Bearbeiten löscht NextPad sie. Wer über Moodle angemeldet ist, bekommt zwei Wochen vorher eine Mail mit dem Inhalt als PDF und kann das Pad um ein Jahr verlängern. Unter „Teilnehmende“ lässt sich das eigene Pad auch selbst verlängern oder löschen.</p>", state.config.retention_days),
        _ => "<p>Pads aus Moodle bleiben, solange es die Aktivität im Kurs gibt. Pads von der Startseite gehören der Person, die sie angelegt hat; sie kann ihr Pad unter „Teilnehmende“ selbst löschen. Eine automatische Löschfrist ist derzeit nicht eingeschaltet.</p>".to_string(),
    };
    let content = format!(r#"<main id="inhalt" class="doc" tabindex="-1"><p><a href="{base}/">← Zur Startseite</a></p><h1>Hilfe zu NextPad</h1>
<p>NextPad ist der Nachfolger von Etherpad an der Hochschule Düsseldorf. Mehrere Personen schreiben dort gleichzeitig am selben Text. Alle sehen die Änderungen der anderen sofort.</p>
<h2>Einem Pad beitreten</h2><p>Den Namen des Pads eingeben und „Öffnen oder anlegen“ wählen. Gleicher Name bedeutet gleiches Pad. Gibt es den Namen noch nicht, legt NextPad ein neues, leeres Pad an. Erlaubt sind Buchstaben, Ziffern, Leerzeichen und die Zeichen - _ . ( ) , bis 50 Zeichen.</p>
<h2>Mit Vorlage beginnen</h2><p>Die Vorlagen legen ein Pad mit zufälligem Namen an und bringen eine Gliederung für Gruppenarbeit oder Protokoll mit. Über „Teilen“ im Pad lässt sich der Link kopieren und an die Gruppe schicken.</p>
<h2>Quiz-Raster</h2><p>„Quiz-Raster“ legt ein Quiz nach Kategorien und Punktwerten an. Wer es anlegt, leitet: Fragen als Tabelle <code>Kategorie | Punkte | Frage | Antwort</code> einfügen, Teams eintragen, am Beamer ein Feld wählen, die Antwort aufdecken und Punkte vergeben. Wer den Link oder den QR-Code hat, sieht Raster, Frage und Punktestand mit, aber nicht die Antworten, bevor sie aufgedeckt sind. Tasten für die Leitung: Leertaste deckt auf, Esc führt zurück zum Raster, Z nimmt die letzte Wertung zurück. Gespielte Felder sind gesperrt; „Rückgängig“ holt die letzte Frage zurück. Nach der letzten Frage zeigt das Quiz die Platzierung und die Fragen, die niemand oder erst nach einem Fehlversuch gelöst hat, zum Nachbesprechen.</p>
<p>Buzzer: Wer mitsieht, wählt auf dem Handy sein Team. Solange eine Frage offen ist und die Antwort nicht aufgedeckt, erscheint ein großer Buzzer. Gewertet wird, wann gedrückt wurde, nicht wann der Druck ankommt: Das Handy gleicht dazu laufend seine Uhr mit dem Server ab. Eine halbe Sekunde nach dem ersten Druck steht die Reihenfolge fest; liegen zwei Teams weniger als 50 ms auseinander, steht „gleichauf“ daneben. Je Handy und Frage zählt ein Druck. Die Punkte vergibt weiterhin die Leitung.</p>
<p>„Buzzer-Messung“ (für die Leitung) prüft, ob ein Buzzer auf dem Handy im Raum fair wäre: Alle öffnen den QR-Code oder tippen im Quiz oben rechts auf „An der Messung teilnehmen“ (der Knopf erscheint, solange die Leitung die Messseite offen hat) und dann auf „Mitmessen“, die Leitung sendet einige Signale, alle drücken, sobald „Jetzt“ erscheint. Die Seite zeigt Laufzeiten, wie genau sich die Uhren der Handys abgleichen lassen, und ein Urteil. Gespeichert werden nur Zeiten und die Art des Geräts, keine Namen; die Leitung kann die Daten als CSV sichern und löschen.</p>
<h2>Ideensammlung</h2><p>„Ideensammlung“ legt eine Liste an, in die alle ohne Namen Ideen oder Fragen eintragen und für Beiträge stimmen. Jede Person hat je Beitrag eine Stimme. Wer die Sammlung angelegt hat, kann oben die Frage setzen, Beiträge als erledigt markieren oder ausblenden. „Link zum Mitmachen kopieren“ holt die Adresse für die Gruppe. „Leinwand öffnen“ zeigt die beliebtesten Beiträge groß für den Beamer, bei freien Sammlungen mit QR-Code zum Mitmachen. Gesichert wird als Markdown oder CSV. Im Moodle-Kurs gibt es die Ideensammlung auch als Aktivität: Dann kommen nur Kursmitglieder hinein, und Lehrende moderieren.</p>
<h2>Zeichenbrett</h2><p>„Zeichenbrett“ legt eine gemeinsame Zeichenfläche an: Formen, Pfeile, Freihand und Text, alle sehen die Zeiger der anderen. Gesichert wird über das Menü links oben als PNG, SVG oder Excalidraw-Datei. Bilder einfügen ist noch nicht möglich. Die Zeichenfläche ist mit Screenreadern kaum nutzbar; für zugängliche Zusammenarbeit eignet sich ein Text-Pad.</p>
<h2>Aus Moodle gestartet</h2><p>Pads, Quiz und Ideensammlungen, die als Aktivität aus Moodle gestartet wurden, zeigen oben „Zurück zum Kurs“. Der Verweis führt auf die Kursseite in Moodle, auch aus der eingebetteten Ansicht heraus.</p>
<h2>Name und Profilbild</h2><p>Ohne Anmeldung heißt man im Pad „Gast“. Unter „Teilnehmende“ lässt sich ein eigener Name eintragen. Wer sich über Moodle anmeldet, erscheint mit Namen und Profilbild aus Moodle.</p>
<h2>Frühere Stände</h2><p>Die Zeitleiste zeigt, wie der Text entstanden ist, und spielt die Änderungen ab.</p>
<h2>Exportieren</h2><p>Über „Export“ gibt es den Text als HTML, Markdown, reinen Text, als PDF oder als Druckansicht.</p>
<h2>Markdown</h2><p>Eingefügter Markdown-Text wird formatiert übernommen, etwa Überschriften mit #, Listen mit -, Aufgaben mit - [ ], **fett** und Tabellen mit senkrechten Strichen (| A | B |, darunter | --- | --- |).</p>
<h2>Einrücken</h2><p>Tab rückt den Absatz oder die Überschrift um eine Stufe ein, Umschalt+Tab wieder aus; dasselbe tun die Knöpfe „Einrücken“ und „Ausrücken“. Markierte Absätze rücken gemeinsam. In Listen verschachtelt Tab den Punkt weiter, in Tabellen springt Tab zur nächsten Zelle.</p>
<h2>Tabellen</h2><p>Das Tabellen-Symbol fügt eine Tabelle mit drei Zeilen, drei Spalten und Kopfzeile ein. Steht der Cursor in einer Tabelle, lassen sich über die Auswahl „Tabelle“ Zeilen und Spalten anfügen oder löschen und die Kopfzeile ein- und ausschalten. Mit der Tabulatortaste geht es zur nächsten Zelle.</p>
<h2>Bewertung</h2><p>Das Daumen-Symbol setzt an der Cursorstelle eine Bewertung mit Daumen hoch und Daumen runter ein, etwa hinter einen Vorschlag. Jede Person hat je Bewertung eine Stimme; ein zweiter Klick nimmt sie zurück, ein Klick auf den anderen Daumen ändert sie. Die Zähler erscheinen auch im Export. Anders als in der Ideensammlung zählt hier der Browser die Stimmen, für verbindliche Abstimmungen eignet sich die Ideensammlung oder Moodle.</p>
<h2>Inhaltsverzeichnis</h2><p>Das H-Symbol rechts in der Werkzeugleiste blendet links ein Inhaltsverzeichnis aus allen Überschriften ein; ein Klick springt zur Stelle. „Ins Pad einfügen“ setzt das Verzeichnis zusätzlich in den Text. Es aktualisiert sich selbst und erscheint in HTML, Markdown und PDF, im PDF mit Seitenzahlen.</p>
<h2>Wer kann ein Pad sehen?</h2><p>Alle, die den Namen oder Link kennen. Pads von der Startseite sind nicht durch ein Passwort geschützt. Für Kurse mit festen Gruppen eignet sich die Pad-Aktivität in Moodle; dort sehen nur Kursmitglieder die Pads.</p>
<h2>Wie lange bleibt ein Pad?</h2>{retention}
<h2>Zuletzt geöffnet</h2><p>Die Liste auf der Startseite speichert nur dieser Browser. Sie lässt sich dort leeren.</p>
<h2>Über NextPad</h2><p>Idee, Konzept und Entwicklung: Christian-Maximilian Steier. NextPad ist freie Software unter der EUPL-1.2, der Quellcode liegt auf <a href="https://github.com/christiansteier/nextpad">GitHub</a>.</p>
</main>"#);
    start_headers(&state, Html(hsd_page(&state, "NextPad · Hilfe", "", "", &content, "")).into_response())

}

/// Abmelden per Adresse (GET): nicht sofort abmelden (Vorab-Laden durch Browser), sondern nachfragen.
pub async fn logout_confirm(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let base = &state.config.base_path;
    let text = match login_author(&state, &headers) {
        Some(a) => format!(r#"<p>Angemeldet über Moodle als <strong>{}</strong>.</p><form method="post" action="{base}/logout"><button type="submit" class="btn btn-primary">Abmelden</button></form>"#, html_escape::encode_text(&a.name)),
        None => "<p>Hier ist niemand angemeldet.</p>".to_string(),
    };
    let content = format!(r#"<main id="inhalt" class="doc" tabindex="-1"><h1>Abmelden</h1>{text}<p><a href="{base}/">Zur Startseite</a></p></main>"#);
    start_headers(&state, Html(hsd_page(&state, "NextPad · Abmelden", "", "", &content, "")).into_response())
}

/// Besitzer-Aktionen: nur per POST von der eigenen Seite, nur wer das freie Pad angelegt hat.
fn owner_check(state: &AppState, headers: &HeaderMap, id: &str) -> Result<String, Response> {
    if !origin_ok(state, headers) || headers.get(header::ORIGIN).is_none() { return Err(StatusCode::FORBIDDEN.into_response()); }
    let access = check_access(state, headers, id).map_err(|(s, m)| (s, m).into_response())?;
    let aid = access.author.map(|a| a.0).unwrap_or_default();
    if !access.free || access.readonly || aid.is_empty() || state.store.owner_of(&access.pad.id).ok().flatten().as_deref() != Some(aid.as_str()) {
        return Err((StatusCode::FORBIDDEN, "Nur wer das Pad angelegt hat, darf das.").into_response());
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
pub async fn extend_page(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let base = &state.config.base_path;
    let t = q.get("t").cloned().unwrap_or_default();
    if t.len() != 48 || !t.chars().all(|c| c.is_ascii_alphanumeric()) || state.store.pad_by_extend_token(&t).ok().flatten().is_none() {
        return start_headers(&state, error_page(StatusCode::NOT_FOUND, "Dieser Link gilt nicht mehr. Vielleicht wurde das Pad schon verlängert oder geöffnet."));
    }
    let icons = icon_links();
    let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad · Verlängern</title>
<style>body{{font-family:Arial,sans-serif;max-width:32em;margin:0 auto;padding:32px 16px;line-height:1.5}} button{{min-height:48px;padding:0 20px;background:#262626;color:#fff;border:0;font:inherit;font-weight:700;cursor:pointer}} h1{{font-size:24px;border-bottom:2px solid #e60028;padding-bottom:6px}}</style></head>
<body><h1>Pad verlängern</h1><p>Das Pad bleibt ein weiteres Jahr erhalten.</p><form method="post" action="{base}/verlaengern"><input type="hidden" name="t" value="{t}"><button type="submit">Um ein Jahr verlängern</button></form></body></html>"#);
    start_headers(&state, Html(body).into_response())
}

pub async fn extend_by_token(State(state): State<Arc<AppState>>, axum::Form(f): axum::Form<HashMap<String, String>>) -> Response {
    let t = f.get("t").cloned().unwrap_or_default();
    let pad = if t.len() == 48 && t.chars().all(|c| c.is_ascii_alphanumeric()) { state.store.pad_by_extend_token(&t).ok().flatten() } else { None };
    let Some(pad) = pad else { return start_headers(&state, error_page(StatusCode::NOT_FOUND, "Dieser Link gilt nicht mehr.")) };
    match crate::retention::extend(&state, &pad) {
        Ok(until) => {
            let base = &state.config.base_path;
            let icons = icon_links();
            let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad · Verlängert</title><style>body{{font-family:Arial,sans-serif;max-width:32em;margin:0 auto;padding:32px 16px;line-height:1.5}}</style></head>
<body><h1>Verlängert</h1><p>Das Pad bleibt bis mindestens {} erhalten.</p><p><a href="{base}/p/{}">Zum Pad</a></p></body></html>"#, crate::pdf::date_de(until), urlencoding(&pad));
            start_headers(&state, Html(body).into_response())
        }
        Err(_) => start_headers(&state, error_page(StatusCode::INTERNAL_SERVER_ERROR, "Verlängern hat nicht geklappt.")),
    }
}


/// PDF eines Pads im eingestellten Design (Typst), Rückfall auf das schlichte PDF.
pub async fn pad_pdf(state: &Arc<AppState>, pad_id: &str, shown_id: &str) -> anyhow::Result<Vec<u8>> {
    let body = state.rooms.typst(pad_id).await?;
    let md = state.rooms.markdown(pad_id).await?;
    let title = md.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or(shown_id).chars().take(80).collect::<String>();
    let title = html_escape::decode_html_entities(&title).to_string();
    let header = format!("NextPad · Stand {}", crate::pdf::date_de(ids::now_ms()));
    let footer = format!("Pad {shown_id}");
    let st = state.clone();
    let res = tokio::task::spawn_blocking(move || {
        let look = crate::look::get(&st);
        let accent = look.palette().accent;
        let o = crate::typeset::Options { hsd: theme_hsd(&st), theme_dir: st.config.theme_dir.as_deref(), logo: crate::look::logo(&st), accent: &accent, product: look.product(), header: &header, footer: &footer, title: &title };
        crate::typeset::pdf(&o, &body)
    }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    match res {
        Ok(b) => Ok(b),
        Err(e) => { tracing::warn!("Typst-Satz gescheitert, schlichtes PDF: {e}"); Ok(crate::pdf::from_markdown(&format!("NextPad · Pad {shown_id}"), &md)) }
    }
}

/// Zeichenbrett-Bündel (Excalidraw, Teile, Schriften), beim Bau eingebettet.
static BOARD: include_dir::Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/static/board");

pub fn board_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for f in ["board.js", "board.css"] { if let Some(file) = BOARD.get_file(f) { for b in file.contents() { h ^= *b as u64; h = h.wrapping_mul(1099511628211); } } }
        format!("{h:016x}")
    })
}

pub async fn static_board(Path(path): Path<String>) -> Response {
    if path.contains("..") { return StatusCode::NOT_FOUND.into_response(); }
    let Some(f) = BOARD.get_file(&path) else { return StatusCode::NOT_FOUND.into_response() };
    let ct = match path.rsplit('.').next().unwrap_or("") {
        "js" => "text/javascript; charset=utf-8", "css" => "text/css; charset=utf-8", "woff2" => "font/woff2", "woff" => "font/woff",
        "ttf" => "font/ttf", "json" => "application/json", "svg" => "image/svg+xml", "png" => "image/png", _ => "application/octet-stream",
    };
    // Teile tragen einen Hash im Namen; board.js/css werden per ?v= versioniert
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "public, max-age=31536000, immutable"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], f.contents()).into_response()
}

pub const START_CSS: &str = include_str!("../static/start.css");
pub async fn static_start_css() -> Response { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], START_CSS).into_response() }
