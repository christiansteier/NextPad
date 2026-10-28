//! Quiz-Raster: Raster aus Kategorien und Punktwerten, die Leitung steuert am Beamer, Teams sammeln Punkte.
//! Fragen und Antworten liegen nur in SQLite, nie im Yjs-Dokument. Eine Antwort verlässt den Server nur zur Leitung
//! oder nachdem die Leitung sie aufgedeckt hat. Wer nicht leitet, sieht Raster, offene Frage und Punktestand, nur lesend.
use crate::{store::QuizField, web, AppState};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, Html, IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::broadcast;

const MAX_FIELDS: usize = 60;
const MAX_CATS: usize = 8;
const MAX_TEAMS: usize = 12;
const MAX_TITLE: usize = 120;
const MAX_CAT: usize = 60;
const MAX_TEAM: usize = 40;
const MAX_QUESTION: usize = 500;
const MAX_ANSWER: usize = 300;

pub const QUIZ_JS: &str = include_str!("../static/quiz.js");
pub const QUIZ_CSS: &str = include_str!("../static/quiz.css");

pub fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for b in QUIZ_JS.bytes().chain(QUIZ_CSS.bytes()) { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        format!("{h:016x}")
    })
}

/// `lead`: darf leiten (wer das freie Quiz angelegt hat, in Kursen die Lehrenden).
pub(crate) struct Viewer { pub pad: String, pub lead: bool }

pub(crate) fn viewer(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Viewer, Response> {
    let access = web::check_access(state, headers, id).map_err(|(s, m)| (s, m).into_response())?;
    if !state.store.is_quiz(&access.pad.id) { return Err((StatusCode::NOT_FOUND, "Das ist kein Quiz-Raster.").into_response()); }
    let author = access.author.map(|a| a.0);
    let creator = access.free && !access.readonly && author.is_some() && state.store.owner_of(&access.pad.id).ok().flatten() == author;
    let moderator = !access.readonly && author.as_deref().is_some_and(|a| state.store.is_moderator(&access.pad.id, a));
    Ok(Viewer { pad: access.pad.id, lead: creator || moderator })
}

/// Schreibende Aufrufe: nur die Leitung, nur von der eigenen Seite (Origin).
pub(crate) fn lead(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Viewer, Response> {
    if !web::origin_ok(state, headers) || headers.get(header::ORIGIN).is_none() { return Err(StatusCode::FORBIDDEN.into_response()); }
    let v = viewer(state, headers, id)?;
    if !v.lead { return Err((StatusCode::FORBIDDEN, "Nur die Leitung des Quiz darf das.").into_response()); }
    Ok(v)
}

fn clean(s: &str, max: usize) -> String {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    s.trim().chars().take(max).collect::<String>().trim_end().to_string()
}

fn no_store(mut r: Response) -> Response {
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// Fragen aus einer Tabelle `Kategorie | Punkte | Frage | Antwort` (Markdown oder mit Tabulatoren aus Tabellenprogrammen).
/// Kopfzeile und Trennzeile werden übersprungen; eine Antwort darf selbst `|` enthalten.
pub fn parse_table(text: &str) -> Result<Vec<QuizField>, String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() { continue; }
        let cells: Vec<String> = if line.contains('\t') { line.split('\t').map(|c| c.trim().to_string()).collect() }
            else { line.trim_start_matches('|').trim_end_matches('|').split('|').map(|c| c.trim().to_string()).collect() };
        if cells.iter().all(|c| c.is_empty() || c.trim_matches(':').chars().all(|ch| ch == '-')) { continue; }
        if cells[0].eq_ignore_ascii_case("kategorie") { continue; }
        if cells.len() < 4 { return Err(format!("Zeile hat weniger als vier Spalten: „{}“", clean(line, 80))); }
        let digits: String = cells[1].chars().filter(|c| c.is_ascii_digit()).collect();
        let points: i64 = digits.parse().ok().filter(|p| *p > 0 && *p <= 100_000).ok_or_else(|| format!("Punkte sind keine Zahl: „{}“", clean(line, 80)))?;
        let (cat, question, answer) = (clean(&cells[0], MAX_CAT), clean(&cells[2], MAX_QUESTION), clean(&cells[3..].join(" | "), MAX_ANSWER));
        if cat.is_empty() || question.is_empty() { return Err(format!("Kategorie oder Frage fehlt: „{}“", clean(line, 80))); }
        out.push(QuizField { id: out.len() as i64 + 1, cat, points, question, answer });
        if out.len() > MAX_FIELDS { return Err(format!("Höchstens {MAX_FIELDS} Fragen.")); }
    }
    if out.is_empty() { return Err("Keine Fragen gefunden.".into()); }
    if out.iter().map(|f| f.cat.as_str()).collect::<HashSet<_>>().len() > MAX_CATS { return Err(format!("Höchstens {MAX_CATS} Kategorien.")); }
    Ok(out)
}

fn table_text(fields: &[QuizField]) -> String {
    let esc = |s: &str| s.replace('|', "/");
    let mut t = String::from("| Kategorie | Punkte | Frage | Antwort |\n| --- | --- | --- | --- |\n");
    for f in fields { t.push_str(&format!("| {} | {} | {} | {} |\n", esc(&f.cat), f.points, esc(&f.question), f.answer)); }
    t
}

/// Gespielt ist ein Feld nach einer Plus-Wertung oder „niemand“; Minus lässt es offen.
fn played(log: &[(i64, Option<i64>, i64)]) -> HashSet<i64> {
    log.iter().filter(|(_, team, delta)| team.is_none() || *delta > 0).map(|(f, _, _)| *f).collect()
}

/// Stand für diese Person. Antworten nur für die Leitung oder nach dem Aufdecken.
pub async fn get(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match viewer(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let (Ok(st), Ok(fields), Ok(teams), Ok(log)) = (state.store.quiz_state(&v.pad), state.store.quiz_fields(&v.pad), state.store.quiz_teams(&v.pad), state.store.quiz_log(&v.pad))
        else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let done = played(&log);
    let teams_json: Vec<_> = teams.iter().map(|(tid, name)| json!({ "id": tid, "name": name, "score": log.iter().filter(|(_, t, _)| *t == Some(*tid)).map(|(_, _, d)| d).sum::<i64>() })).collect();
    // Gespielte Fragen kennen alle schon; „schwer“ = niemand gelöst oder mit Abzug, für die Nachbesprechung am Ende
    let board: Vec<_> = fields.iter().map(|f| {
        let mut b = json!({ "id": f.id, "cat": f.cat, "points": f.points, "played": done.contains(&f.id) });
        if done.contains(&f.id) {
            b["question"] = json!(f.question);
            b["schwer"] = json!(log.iter().any(|(fid, t, d)| *fid == f.id && (t.is_none() || *d < 0)));
        }
        b
    }).collect();
    let current = st.current.and_then(|c| fields.iter().find(|f| f.id == c)).map(|f| {
        let mut q = json!({ "id": f.id, "cat": f.cat, "points": f.points, "question": f.question, "revealed": st.revealed });
        if v.lead || st.revealed { q["answer"] = json!(f.answer); }
        q
    });
    let mut out = json!({ "title": st.title, "lead": v.lead, "teams": teams_json, "board": board, "current": current });
    if v.lead {
        out["canUndo"] = json!(!log.is_empty());
        out["setup"] = json!({ "title": st.title, "teams": teams.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "), "table": if fields.is_empty() { String::new() } else { table_text(&fields) } });
    }
    no_store(Json(out).into_response())
}

/// Fragen und Teams setzen; Punkte und offene Frage beginnen neu.
pub async fn setup(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let s = |k: &str| body.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let fields = match parse_table(&s("table")) { Ok(f) => f, Err(m) => return (StatusCode::BAD_REQUEST, m).into_response() };
    let teams: Vec<String> = s("teams").split(',').map(|t| clean(t, MAX_TEAM)).filter(|t| !t.is_empty()).collect();
    if teams.is_empty() { return (StatusCode::BAD_REQUEST, "Bitte mindestens ein Team eintragen.").into_response(); }
    if teams.len() > MAX_TEAMS { return (StatusCode::BAD_REQUEST, format!("Höchstens {MAX_TEAMS} Teams.")).into_response(); }
    let title = clean(&s("title"), MAX_TITLE);
    match state.store.quiz_replace(&v.pad, &title, &fields, &teams) {
        Ok(()) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true, "fields": fields.len() })).into_response() }
        Err(e) => { tracing::warn!("Quiz einrichten: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Frage öffnen, Antwort aufdecken oder zurück zum Raster (`field: null`).
pub async fn open(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let field = body.get("field").and_then(|x| x.as_i64());
    let revealed = body.get("revealed").and_then(|x| x.as_bool()).unwrap_or(false);
    // Gespielte Fragen nicht versehentlich wieder öffnen; zurück holt sie nur „Rückgängig“
    if let Some(f) = field {
        match state.store.quiz_log(&v.pad) {
            Ok(log) if played(&log).contains(&f) => return (StatusCode::CONFLICT, "Diese Frage ist schon gespielt. „Rückgängig“ holt sie zurück.").into_response(),
            Ok(_) => {}
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    match state.store.quiz_open(&v.pad, field, revealed) {
        Ok(true) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => { tracing::warn!("Quiz öffnen: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Wertung: `team` und `plus` (true/false); ohne Team gilt das Feld als „niemand“. Die Punkte kommen aus dem Feld.
pub async fn score(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let Some(field) = body.get("field").and_then(|x| x.as_i64()) else { return StatusCode::BAD_REQUEST.into_response() };
    let team = body.get("team").and_then(|x| x.as_i64());
    let plus = body.get("plus").and_then(|x| x.as_bool()).unwrap_or(true);
    let points = match state.store.quiz_fields(&v.pad) { Ok(f) => f.iter().find(|f| f.id == field).map(|f| f.points), Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let Some(points) = points else { return StatusCode::NOT_FOUND.into_response() };
    let delta = if team.is_none() { 0 } else if plus { points } else { -points };
    match state.store.quiz_score(&v.pad, field, team, delta) {
        Ok(true) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => { tracing::warn!("Quiz werten: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn undo(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    match state.store.quiz_undo(&v.pad) {
        Ok(_) => { state.ideas.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Err(e) => { tracing::warn!("Quiz rückgängig: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Live-Meldungen: nur „geändert“, ohne Inhalt; den Stand holt jede Seite mit ihren eigenen Rechten.
/// Nutzt den Kanal der Ideensammlung, Pad-Kennungen sind eindeutig.
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

/// Export für alle mit dem Link: nur Titel und Punktestand, keine Fragen und keine Antworten.
pub fn export(state: &AppState, pad: &str, format: &str) -> Option<Response> {
    if !matches!(format, "md" | "txt") { return None; }
    let st = state.store.quiz_state(pad).ok()?;
    let teams = state.store.quiz_teams(pad).ok()?;
    let log = state.store.quiz_log(pad).ok()?;
    let mut rows: Vec<(String, i64)> = teams.iter().map(|(id, n)| (n.clone(), log.iter().filter(|(_, t, _)| *t == Some(*id)).map(|(_, _, d)| d).sum())).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = format!("# {}\n\nPunktestand\n\n", if st.title.is_empty() { "Quiz-Raster" } else { &st.title });
    for (n, p) in rows { out.push_str(&format!("- {n}: {p}\n")); }
    let ct = if format == "md" { "text/markdown; charset=utf-8" } else { "text/plain; charset=utf-8" };
    Some(([(header::CONTENT_TYPE, ct)], out).into_response())
}

/// QR-Code zum Mitsehen: nur bei freien Quiz über den Link selbst (nicht den Leselink, der Rechte hätte ohnehin keine).
/// `suffix`: Unterseite, auf die der Code führt (leer: das Quiz selbst).
pub fn join_svg(state: &AppState, headers: &HeaderMap, free: bool, url_id: &str, pad_id: &str, suffix: &str) -> String {
    if !free || url_id.starts_with("r.") { return String::new(); }
    let base = &state.config.base_path;
    let origin = state.config.public_url.clone().unwrap_or_else(|| {
        let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
        format!("https://{host}{base}")
    });
    let url = format!("{}/p/{}{suffix}", origin.trim_end_matches('/'), web::urlencoding(pad_id));
    let qr = qrcode::QrCode::new(url.as_bytes()).map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(320, 320).quiet_zone(true).build()).unwrap_or_default();
    let qr = qr.split_once("<svg").map(|(_, r)| format!(r#"<svg role="img" aria-label="QR-Code zum Mitsehen"{r}"#)).unwrap_or_default();
    format!(r#"{qr}<p>Mitsehen auf dem Handy:<br><strong>{}</strong></p>"#, html_escape::encode_text(&url))
}

/// Seite des Quiz; Leitung und Mitsehende bekommen dieselbe Seite, das Skript zeigt je nach Rolle.
pub fn page(state: &AppState, url_id: &str, cfg: serde_json::Value, join: &str) -> Response {
    let base = &state.config.base_path;
    let content = format!(r#"<main id="inhalt" class="quiz" tabindex="-1">
<div class="quiz-top"><div><p class="kicker" id="quiz-kicker">Quiz-Raster</p><h1 id="quiz-title">Quiz-Raster</h1></div>
<div class="quiz-tools" id="lead-tools" hidden>
<button type="button" class="btn btn-secondary" id="q-undo">Rückgängig</button>
<button type="button" class="btn btn-secondary" id="q-qr" aria-controls="qr-box" aria-expanded="false">QR-Code</button>
<button type="button" class="btn btn-secondary" id="q-full">Vollbild</button>
<button type="button" class="btn btn-secondary" id="q-edit" aria-controls="setup" aria-expanded="false">Fragen bearbeiten</button>
<a class="btn btn-secondary" id="q-messung" href="{base}/p/{u}/messung">Buzzer-Messung</a>
</div></div>
<section class="panel quiz-setup" id="setup" hidden aria-labelledby="setup-h">
<h2 id="setup-h">Fragen und Teams</h2>
<label class="field-label" for="in-title">Titel</label><input id="in-title" maxlength="{MAX_TITLE}" autocomplete="off">
<label class="field-label" for="in-teams">Teams, durch Komma getrennt</label><input id="in-teams" autocomplete="off" placeholder="Team 1, Team 2, Team 3">
<label class="field-label" for="in-table">Fragen als Tabelle: Kategorie | Punkte | Frage | Antwort</label>
<textarea id="in-table" rows="12" spellcheck="false" aria-describedby="table-hint setup-error"></textarea>
<p class="hint" id="table-hint">Eine Zeile je Frage, als Markdown-Tabelle oder aus einem Tabellenprogramm kopiert. Bis {MAX_CATS} Kategorien und {MAX_FIELDS} Fragen. Die Antworten sieht nur die Leitung, bis sie aufgedeckt werden.</p>
<p class="field-error" id="setup-error" role="alert" hidden></p>
<p><button type="button" class="btn btn-primary" id="q-save">Speichern und neu beginnen</button> <span class="hint">Punkte beginnen wieder bei null.</span></p>
</section>
<aside class="panel quiz-qr" id="qr-box" hidden>{join}</aside>
<section class="quiz-board" id="board" aria-label="Raster"></section>
<section class="quiz-question" id="question" hidden aria-live="polite">
<p class="quiz-meta" id="q-meta"></p><p class="quiz-q" id="q-text"></p><p class="quiz-a" id="q-answer" hidden></p>
<div class="quiz-actions" id="q-actions"></div>
</section>
<section class="quiz-ende" id="ende" hidden aria-live="polite" aria-labelledby="ende-h"></section>
<p class="hint quiz-empty" id="empty" hidden></p>
<section class="quiz-scores" id="scores" aria-label="Punktestand"></section>
<noscript><p class="notice">Das Quiz-Raster braucht JavaScript.</p></noscript>
<p class="hint quiz-export"><a href="{base}/p/{u}/export/md">Punktestand sichern</a></p>
</main>
<script type="application/json" id="nextpad-config">{cfg}</script>"#,
        u = web::urlencoding(url_id), cfg = cfg.to_string().replace("</", "<\\/"));
    let head = format!(r#"<link rel="stylesheet" href="{base}/static/quiz.css?v={v}">"#, v = version());
    let scripts = format!(r#"<script src="{base}/static/quiz.js?v={v}"></script>"#, v = version());
    let html = web::page_frame(state, "NextPad · Quiz-Raster", &head, &content, &scripts);
    let mut resp = Html(html).into_response();
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors {}; base-uri 'none'", state.config.frame_ancestors);
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    no_store(resp)
}

pub async fn static_js() -> Response { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], QUIZ_JS).into_response() }
pub async fn static_css() -> Response { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], QUIZ_CSS).into_response() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabelle_markdown_und_tabulator() {
        let f = parse_table("| Kategorie | Punkte | Frage | Antwort |\n| --- | --- | --- | --- |\n| Audio | 100 | Wie lang? | 5 Min. |\n| Audio | 200 | Was? | a | b |").unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].answer, "a | b");
        let t = parse_table("KI\t300\tWelcher Dienst?\tKI:connect.nrw").unwrap();
        assert_eq!((t[0].cat.as_str(), t[0].points), ("KI", 300));
    }
    #[test]
    fn tabelle_fehler() {
        assert!(parse_table("").is_err());
        assert!(parse_table("| a | b |").is_err());
        assert!(parse_table("| a | x | Frage | Antwort |").is_err());
    }
    #[test]
    fn gespielt_nur_nach_plus_oder_niemand() {
        let p = played(&[(1, Some(1), -100), (2, Some(2), 200), (3, None, 0)]);
        assert!(!p.contains(&1) && p.contains(&2) && p.contains(&3));
    }
}
