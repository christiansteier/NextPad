//! Ideensammlung: Ideen und Fragen sammeln und abstimmen (nach dem Vorbild von liveask).
//! Der Server prüft jede Stimme: eine je Person und Beitrag, moderieren darf nur, wer die Sammlung angelegt hat.
//! Beiträge erscheinen ohne Namen. Änderungen gehen per Server-Sent Events an alle offenen Seiten.
use crate::{web, AppState};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, Html, IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{collections::HashMap, sync::{Arc, Mutex}};
use tokio::sync::broadcast;

pub const MAX_TEXT: usize = 500;
const MAX_PROMPT: usize = 200;
const MAX_TOTAL: i64 = 500;
const MAX_OWN: i64 = 30;

pub const IDEEN_JS: &str = include_str!("../static/ideen.js");
pub const IDEEN_CSS: &str = include_str!("../static/ideen.css");
pub const WAND_JS: &str = include_str!("../static/ideen-wand.js");

pub fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for b in IDEEN_JS.bytes().chain(IDEEN_CSS.bytes()).chain(WAND_JS.bytes()) { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        format!("{h:016x}")
    })
}

/// Je Sammlung ein Kanal; offene Seiten holen nach jeder Meldung den Stand neu.
#[derive(Default)]
pub struct Hub(Mutex<HashMap<String, broadcast::Sender<()>>>);

impl Hub {
    pub fn subscribe(&self, pad: &str) -> broadcast::Receiver<()> {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, tx| tx.receiver_count() > 0);
        m.entry(pad.to_string()).or_insert_with(|| broadcast::channel(16).0).subscribe()
    }
    pub fn notify(&self, pad: &str) {
        if let Some(tx) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(pad) { let _ = tx.send(()); }
    }
}

/// `owner`: darf moderieren (wer die freie Sammlung angelegt hat, in Kursen die Lehrenden);
/// `can_delete`: darf die ganze Sammlung löschen (nur freie Sammlungen, Kursaktivitäten verwaltet Moodle).
struct Viewer { pad: String, author: Option<String>, readonly: bool, owner: bool, can_delete: bool }

fn viewer(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Viewer, Response> {
    let access = web::check_access(state, headers, id).map_err(|(s, m)| (s, m).into_response())?;
    if !state.store.is_ideas(&access.pad.id) { return Err((StatusCode::NOT_FOUND, "Das ist keine Ideensammlung.").into_response()); }
    let author = access.author.map(|a| a.0);
    let creator = access.free && !access.readonly && author.is_some() && state.store.owner_of(&access.pad.id).ok().flatten() == author;
    let moderator = !access.readonly && author.as_deref().is_some_and(|a| state.store.is_moderator(&access.pad.id, a));
    Ok(Viewer { pad: access.pad.id, author, readonly: access.readonly, owner: creator || moderator, can_delete: creator })
}

/// Schreibende Aufrufe: nur von der eigenen Seite (Origin) und mit eigener Kennung.
fn writer(state: &AppState, headers: &HeaderMap, id: &str) -> Result<(Viewer, String), Response> {
    if !web::origin_ok(state, headers) || headers.get(header::ORIGIN).is_none() { return Err(StatusCode::FORBIDDEN.into_response()); }
    let v = viewer(state, headers, id)?;
    match (&v.author, v.readonly) {
        (Some(a), false) => { let a = a.clone(); Ok((v, a)) }
        _ => Err((StatusCode::FORBIDDEN, "Über diesen Link lässt sich nur mitlesen.").into_response()),
    }
}

fn clean(s: &str, max: usize) -> String {
    let s: String = s.chars().map(|c| if c == '\r' || c == '\t' { ' ' } else { c }).filter(|c| *c == '\n' || !c.is_control()).collect();
    let s = s.trim();
    s.chars().take(max).collect::<String>().trim_end().to_string()
}

fn no_store(mut r: Response) -> Response {
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// Stand für diese Person: ausgeblendete Beiträge sieht nur, wer moderiert.
pub async fn get(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match viewer(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let (Ok(mut items), Ok(prompt)) = (state.store.ideas(&v.pad), state.store.idea_prompt(&v.pad)) else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    if !v.owner { items.retain(|i| !i.hidden); }
    let (votes, mine) = match &v.author {
        Some(a) => (state.store.my_votes(&v.pad, a).unwrap_or_default(), state.store.mine(&v.pad, a).unwrap_or_default()),
        None => Default::default(),
    };
    no_store(Json(json!({
        "prompt": prompt, "items": items, "voted": votes, "mine": mine,
        "owner": v.owner, "canDelete": v.can_delete, "canWrite": !v.readonly && v.author.is_some(), "maxText": MAX_TEXT,
    })).into_response())
}

pub async fn add(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, author) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    let text = clean(body.get("text").and_then(|t| t.as_str()).unwrap_or(""), MAX_TEXT);
    if text.is_empty() { return (StatusCode::BAD_REQUEST, "Bitte einen Text eingeben.").into_response(); }
    match state.store.add_idea(&v.pad, &author, &text, MAX_TOTAL, MAX_OWN) {
        Ok(Some(n)) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true, "id": n })).into_response() }
        Ok(None) => (StatusCode::TOO_MANY_REQUESTS, "Die Sammlung ist voll oder du hast schon sehr viele Beiträge geschrieben.").into_response(),
        Err(e) => { tracing::warn!("Idee speichern: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn vote(State(state): State<Arc<AppState>>, Path((id, n)): Path<(String, i64)>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, author) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    let on = body.get("on").and_then(|b| b.as_bool()).unwrap_or(true);
    match state.store.vote(&v.pad, n, &author, on) {
        Ok(true) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => (StatusCode::CONFLICT, "Über diesen Beitrag lässt sich nicht mehr abstimmen.").into_response(),
        Err(e) => { tracing::warn!("Stimme: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn moderate(State(state): State<Arc<AppState>>, Path((id, n)): Path<(String, i64)>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, _) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    if !v.owner { return (StatusCode::FORBIDDEN, "Nur wer die Sammlung angelegt hat, darf das.").into_response(); }
    let flag = |k: &str| body.get(k).and_then(|b| b.as_bool());
    match state.store.moderate_idea(&v.pad, n, flag("hidden"), flag("done")) {
        Ok(true) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => { tracing::warn!("Moderation: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn prompt(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, _) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    if !v.owner { return (StatusCode::FORBIDDEN, "Nur wer die Sammlung angelegt hat, darf das.").into_response(); }
    let text = clean(body.get("prompt").and_then(|t| t.as_str()).unwrap_or(""), MAX_PROMPT).replace('\n', " ");
    match state.store.set_idea_prompt(&v.pad, &text) {
        Ok(()) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true, "prompt": text })).into_response() }
        Err(e) => { tracing::warn!("Frage setzen: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Live-Meldungen: nur „geändert“, ohne Inhalt; der Stand kommt über GET mit den Rechten der Person.
pub async fn live(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match viewer(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let rx = state.ideas.subscribe(&v.pad);
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => Some((Ok::<_, std::convert::Infallible>(Event::default().event("change").data("1")), rx)),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });
    let mut r = Sse::new(stream).keep_alive(KeepAlive::default()).into_response();
    r.headers_mut().insert("x-accel-buffering", HeaderValue::from_static("no"));
    no_store(r)
}

/// Export für die Auswertung: Markdown oder CSV, nach Stimmen geordnet, ohne ausgeblendete Beiträge.
pub fn export(state: &AppState, pad: &str, format: &str) -> Option<Response> {
    let mut items = state.store.ideas(pad).ok()?;
    items.retain(|i| !i.hidden);
    items.sort_by(|a, b| b.votes.cmp(&a.votes).then(a.id.cmp(&b.id)));
    let prompt = state.store.idea_prompt(pad).unwrap_or_default();
    match format {
        "md" | "txt" => {
            let mut out = format!("# {}\n\n", if prompt.is_empty() { "Ideensammlung" } else { &prompt });
            for i in &items {
                let mark = if i.done { " (erledigt)" } else { "" };
                out.push_str(&format!("- {} · {} {}{mark}\n", i.text.replace('\n', " "), i.votes, if i.votes == 1 { "Stimme" } else { "Stimmen" }));
            }
            let ct = if format == "md" { "text/markdown; charset=utf-8" } else { "text/plain; charset=utf-8" };
            Some(([(header::CONTENT_TYPE, ct)], out).into_response())
        }
        "csv" => {
            let mut out = String::from("\u{feff}Nr;Beitrag;Stimmen;Erledigt\r\n");
            for i in &items {
                let t = i.text.replace('"', "\"\"").replace('\n', " ");
                // Formeln in Tabellenprogrammen nicht ausführen lassen
                let t = if t.starts_with(['=', '+', '-', '@']) { format!("'{t}") } else { t };
                out.push_str(&format!("{};\"{t}\";{};{}\r\n", i.id, i.votes, if i.done { "ja" } else { "nein" }));
            }
            Some(([(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"ideen.csv\"")], out).into_response())
        }
        _ => None,
    }
}

/// Seite der Sammlung; der Inhalt kommt per Skript, ohne Skript steht ein Hinweis da.
pub fn page(state: &AppState, url_id: &str, cfg: serde_json::Value) -> Response {
    let base = &state.config.base_path;
    let content = format!(r#"<main id="inhalt" class="ideas" tabindex="-1"><div class="wrap">
<section class="panel ideas-head" aria-labelledby="ideas-h">
<p class="kicker" id="view-kicker">Ideensammlung · Bearbeitungsansicht</p>
<div class="title-row"><h1 id="ideas-h"><span id="prompt">Ideen und Fragen</span></h1><button type="button" class="icon-btn" id="prompt-edit" aria-controls="prompt-form" aria-expanded="false" hidden><svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3"/></svg><span class="visually-hidden">Frage ändern</span></button></div>
<form id="prompt-form" class="prompt-form" hidden><label class="field-label" for="prompt-input">Frage oder Thema der Sammlung</label><div class="field-row"><input id="prompt-input" maxlength="{MAX_PROMPT}" autocomplete="off"><button class="btn btn-secondary" type="submit">Speichern</button></div></form>
<form id="add-form" class="add-form" hidden>
<label class="field-label" for="add-text">Deine Idee oder Frage</label>
<textarea id="add-text" maxlength="{MAX_TEXT}" rows="3" required aria-describedby="add-hint add-error"></textarea>
<p class="field-error" id="add-error" role="alert" hidden></p>
<div class="add-row"><p class="hint" id="add-hint">Ohne Namen. <span id="add-count">0</span> von {MAX_TEXT} Zeichen.</p><button class="btn btn-primary" type="submit">Abschicken</button></div>
</form>
<p class="hint" id="readonly-hint" hidden>Über diesen Link lässt sich nur mitlesen.</p>
<noscript><p class="notice">Die Ideensammlung braucht JavaScript.</p></noscript>
</section>
<div class="ideas-bar"><div class="sort" role="group" aria-label="Sortieren"><button type="button" class="tab" data-sort="top" aria-pressed="true">Beliebt</button><button type="button" class="tab" data-sort="new" aria-pressed="false">Neu</button></div><p class="hint" id="ideas-count" aria-live="polite"></p></div>
<ol class="idea-list" id="open-list" aria-label="Offene Beiträge"></ol>
<p class="empty hint" id="empty" hidden>Noch keine Beiträge. Den Link teilen, dann können alle mitmachen.</p>
<details class="idea-group" id="done-box" hidden><summary>Erledigt (<span id="done-n">0</span>)</summary><ol class="idea-list" id="done-list"></ol></details>
<details class="idea-group" id="hidden-box" hidden><summary>Ausgeblendet (<span id="hidden-n">0</span>)</summary><p class="hint">Nur für dich sichtbar.</p><ol class="idea-list" id="hidden-list"></ol></details>
<section class="panel ideas-share" aria-labelledby="share-h">
<h2 id="share-h">Teilen, zeigen, sichern</h2>
<p class="wall-row share-row"><button class="btn btn-secondary" type="button" id="share-copy">Link zum Mitmachen kopieren</button><span class="hint">Wer den Link hat, kann Beiträge eintragen und abstimmen. Ohne Anmeldung und ohne Namen.</span></p>
<p class="wall-row"><a class="btn btn-secondary" href="{base}/p/{u}/leinwand" target="_blank" rel="noopener">Leinwand öffnen</a><span class="hint">Große Ansicht für den Beamer mit den beliebtesten Beiträgen und einem QR-Code zum Mitmachen. Sie aktualisiert sich selbst.</span></p>
<p class="hint">Sichern: <a href="{base}/p/{u}/export/md">Markdown</a> · <a href="{base}/p/{u}/export/csv">CSV für Tabellen</a></p>
<p class="owner-tools" id="owner-tools" hidden><button type="button" class="btn-link" id="delete-all">Sammlung löschen</button></p>
</section>
</div></main>
<script type="application/json" id="nextpad-config">{cfg}</script>"#,
        u = web::urlencoding(url_id), cfg = cfg.to_string().replace("</", "<\\/"));
    let head = format!(r#"<link rel="stylesheet" href="{base}/static/ideen.css?v={v}">"#, v = version());
    let scripts = format!(r#"<script src="{base}/static/ideen.js?v={v}"></script>"#, v = version());
    let html = web::page_frame(state, "NextPad · Ideensammlung", &head, &content, &scripts);
    let mut resp = Html(html).into_response();
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors {}; base-uri 'none'", state.config.frame_ancestors);
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    no_store(resp)
}

/// Leinwand für den Beamer: beliebteste offene Beiträge groß, dazu ein QR-Code zum Mitmachen
/// (nur bei freien Sammlungen und nur über den Link zum Mitmachen, nicht über den Leselink).
pub async fn wall(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match web::check_access(&state, &headers, &id) { Ok(a) => a, Err((s, m)) => return (s, m).into_response() };
    if !state.store.is_ideas(&access.pad.id) { return (StatusCode::NOT_FOUND, "Das ist keine Ideensammlung.").into_response(); }
    let base = &state.config.base_path;
    // nach dem Link, nicht nach dem Cookie: wer die Leinwand direkt in einem frischen Browser öffnet, hat noch keins
    let join = if access.free && !id.starts_with("r.") {
        let origin = state.config.public_url.clone().unwrap_or_else(|| {
            let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
            format!("https://{host}{base}")
        });
        let url = format!("{}/p/{}", origin.trim_end_matches('/'), web::urlencoding(&access.pad.id));
        let qr = qrcode::QrCode::new(url.as_bytes()).map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(260, 260).quiet_zone(true).build()).unwrap_or_default();
        let qr = qr.split_once("<svg").map(|(_, r)| format!(r#"<svg role="img" aria-label="QR-Code zum Mitmachen"{r}"#)).unwrap_or_default();
        format!(r#"<aside class="wall-join">{qr}<p>Mitmachen:<br><strong>{}</strong></p></aside>"#, html_escape::encode_text(&url))
    } else if !access.free {
        match state.store.join_url(&access.pad.id) {
            Some(u) => format!(r#"<aside class="wall-join">{}</aside>"#, crate::quiz::qr_block(&u, "Mitmachen über Moodle:")),
            None => r#"<aside class="wall-join"><p>Mitmachen über die Aktivität im Moodle-Kurs.</p></aside>"#.to_string(),
        }
    } else { String::new() };
    let brand = if state.config.theme == "hsd" && web::has_logo(&state) { format!(r#"<img class="wall-logo" src="{}" alt="{}">"#, web::theme_url(&state, "logo.png"), html_escape::encode_double_quoted_attribute(crate::look::get(&state).institution())) } else { format!(r#"<span class="wall-brand">{}{}</span>"#, web::logo_mark("wall-mark"), html_escape::encode_text(crate::look::get(&state).product())) };
    let cfg = json!({ "urlId": id, "basePath": base });
    let icons = web::icon_links();
    let css = web::look_css(&state);
    let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="color-scheme" content="light dark"><title>Leinwand · NextPad</title>
<link rel="stylesheet" href="{base}/static/start.css?v={sv}"><link rel="stylesheet" href="{base}/static/ideen.css?v={v}">{css}</head>
<body class="wall"><main class="wall-main"><header class="wall-head"><div><p class="kicker">Ideensammlung</p><h1 id="prompt">Ideen und Fragen</h1></div>{brand}</header>
<div class="wall-body"><ol class="wall-list" id="wall-list"></ol>{join}</div>
<p class="wall-empty" id="wall-empty" hidden>Noch keine Beiträge.</p><p class="wall-count" id="wall-count"></p></main>
<script type="application/json" id="nextpad-config">{cfg}</script><script src="{base}/static/ideen-wand.js?v={v}"></script></body></html>"#,
        sv = web::start_version(), v = version(), cfg = cfg.to_string().replace("</", "<\\/"));
    let mut resp = Html(body).into_response();
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; form-action 'none'; frame-ancestors {}; base-uri 'none'", state.config.frame_ancestors);
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    no_store(resp)
}

pub async fn static_wand() -> Response { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], WAND_JS).into_response() }
pub async fn static_js() -> Response { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], IDEEN_JS).into_response() }
pub async fn static_css() -> Response { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], IDEEN_CSS).into_response() }
