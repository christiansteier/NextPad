//! Quiz-Raster: Raster aus Kategorien und Punktwerten, die Leitung steuert am Beamer, Teams sammeln Punkte.
//! Fragen und Antworten liegen nur in SQLite, nie im Yjs-Dokument. Eine Antwort verlässt den Server nur zur Leitung
//! oder nachdem die Leitung sie aufgedeckt hat. Wer nicht leitet, sieht Raster, offene Frage und Punktestand, nur lesend.
use crate::{store::{Buzz, QuizField}, web, AppState};
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
/// Buzzer: nach dem ersten Eingang so lange sammeln, dann steht die Reihenfolge.
const BUZZ_FENSTER_MS: f64 = 500.0;
/// Unter diesem Abstand gelten zwei Teams als gleichauf (Touchscreens streuen selbst so viel).
const BUZZ_GLEICH_MS: f64 = 50.0;
const MAX_BUZZ: i64 = 300;

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

fn jetzt() -> f64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0) }

/// Reihenfolge der Teams: je Team zählt der früheste Druck nach abgeglichener Druckzeit, aber nur Drücke, die bis
/// `BUZZ_FENSTER_MS` nach dem ersten Eingang angekommen sind. Spätere stehen danach, nach Eingang, als „spät“.
/// `fertig`: das Fenster ist vorbei, die Reihenfolge ändert sich nicht mehr.
pub fn buzz_order(buzzes: &[Buzz], teams: &[(i64, String)], now: f64) -> serde_json::Value {
    let Some(first) = buzzes.iter().map(|b| b.ts_ein).min_by(|a, b| a.total_cmp(b)) else { return serde_json::Value::Null };
    let window = first + BUZZ_FENSTER_MS;
    let name = |t: i64| teams.iter().find(|(id, _)| *id == t).map(|(_, n)| n.clone()).unwrap_or_default();
    let mut best: Vec<(i64, f64, bool)> = Vec::new();
    for b in buzzes.iter().filter(|b| b.ts_ein <= window) {
        match best.iter_mut().find(|x| x.0 == b.team) {
            Some(x) if b.t_press < x.1 => { x.1 = b.t_press; x.2 = b.flagged; }
            Some(_) => {}
            None => best.push((b.team, b.t_press, b.flagged)),
        }
    }
    best.sort_by(|a, b| a.1.total_cmp(&b.1));
    let t0 = best.first().map(|x| x.1).unwrap_or(0.0);
    let mut out = Vec::new();
    let mut prev = t0;
    for (i, (team, t, flagged)) in best.iter().enumerate() {
        out.push(json!({ "team": team, "name": name(*team), "abstand": ((t - t0) * 10.0).round() / 10.0, "gleichauf": i > 0 && t - prev < BUZZ_GLEICH_MS, "unsicher": flagged, "spaet": false }));
        prev = *t;
    }
    let mut late: Vec<&Buzz> = buzzes.iter().filter(|b| b.ts_ein > window && !best.iter().any(|x| x.0 == b.team)).collect();
    late.sort_by(|a, b| a.ts_ein.total_cmp(&b.ts_ein));
    for b in late {
        if out.iter().any(|o| o["team"] == json!(b.team)) { continue; }
        out.push(json!({ "team": b.team, "name": name(b.team), "abstand": serde_json::Value::Null, "gleichauf": false, "unsicher": b.flagged, "spaet": true }));
    }
    json!({ "fertig": now >= window, "reihe": out })
}

/// Druckzeit in Serverzeit aus Gerätezeit und Uhrabgleich; unplausibel (nach dem Eingang, vor dem Öffnen oder mehr
/// als Laufzeit plus 1,5 s vor dem Eingang) wird durch Eingang minus halbe Laufzeit ersetzt und markiert.
pub fn buzz_time(t: f64, off: f64, rtt: f64, ein: f64, open: f64) -> (f64, bool) {
    let p = t + off;
    let ok = rtt >= 0.0 && rtt <= 3000.0 && p <= ein + 20.0 && p >= open - 20.0 && ein - p <= rtt + 1500.0;
    if ok { (p, false) } else { (ein - rtt.clamp(0.0, 3000.0) / 2.0, true) }
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
    let buzz = match (st.current, state.store.quiz_buzz_open(&v.pad)) {
        (Some(c), Ok(Some((f, _)))) if c == f => state.store.quiz_buzzes(&v.pad, c).map(|b| buzz_order(&b, &teams, jetzt())).unwrap_or_default(),
        _ => serde_json::Value::Null,
    };
    let mut out = json!({ "title": st.title, "lead": v.lead, "teams": teams_json, "board": board, "current": current, "buzz": buzz });
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
    let before = state.store.quiz_state(&v.pad).ok().and_then(|s| s.current);
    match state.store.quiz_open(&v.pad, field, revealed) {
        Ok(true) => {
            // neu geöffnete Frage: neue Buzzer-Runde
            if let Some(f) = field.filter(|f| !revealed && before != Some(*f)) {
                if let Err(e) = state.store.quiz_buzz_start(&v.pad, f, jetzt()) { tracing::warn!("Buzzer starten: {e}"); }
            }
            state.ideas.notify(&v.pad);
            Json(json!({ "ok": true })).into_response()
        }
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

/// Buzzer-Druck von einem Handy: `dev` (Kennung aus dem Browser), `team`, `field`, Druckzeit `t` in Gerätezeit, dazu
/// Versatz `off` und Laufzeit `rtt` aus dem letzten Uhrabgleich. Nur solange die Frage offen und nicht aufgedeckt ist.
pub async fn buzzer(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let ein = jetzt();
    if !web::origin_ok(&state, &headers) || headers.get(header::ORIGIN).is_none() { return StatusCode::FORBIDDEN.into_response(); }
    let v = match viewer(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let num = |k: &str, max: f64| body.get(k).and_then(|x| x.as_f64()).filter(|x| x.is_finite() && x.abs() <= max);
    let dev = body.get("dev").and_then(|x| x.as_str()).unwrap_or("");
    let (Some(team), Some(field), Some(t), Some(off), Some(rtt)) = (body.get("team").and_then(|x| x.as_i64()), body.get("field").and_then(|x| x.as_i64()), num("t", 1e14), num("off", 1e13), num("rtt", 30_000.0))
        else { return StatusCode::BAD_REQUEST.into_response() };
    if !(8..=32).contains(&dev.len()) || !dev.chars().all(|c| c.is_ascii_alphanumeric()) { return StatusCode::BAD_REQUEST.into_response(); }
    let (Ok(st), Ok(teams), Ok(open)) = (state.store.quiz_state(&v.pad), state.store.quiz_teams(&v.pad), state.store.quiz_buzz_open(&v.pad))
        else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    if !teams.iter().any(|(tid, _)| *tid == team) { return (StatusCode::BAD_REQUEST, "Dieses Team gibt es nicht.").into_response(); }
    let Some((open_field, open_ts)) = open.filter(|(f, _)| st.current == Some(*f) && *f == field && !st.revealed)
        else { return (StatusCode::CONFLICT, "Der Buzzer ist gerade zu.").into_response() };
    let (t_press, flagged) = buzz_time(t, off, rtt, ein, open_ts);
    match state.store.quiz_buzz_add(&v.pad, open_field, dev, &Buzz { team, t_press, ts_ein: ein, flagged }, MAX_BUZZ) {
        Ok(added) => {
            if added { state.ideas.notify(&v.pad); }
            Json(json!({ "ok": true, "neu": added })).into_response()
        }
        Err(e) => { tracing::warn!("Buzzer: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
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
    // Moodle-Aktivität: der Code führt zur Aktivität in Moodle (Anmeldung dort), nicht direkt ins Pad
    if !free {
        return state.store.join_url(pad_id).map(|u| qr_block(&u, "Mitmachen über Moodle:")).unwrap_or_default();
    }
    if url_id.starts_with("r.") { return String::new(); }
    let base = &state.config.base_path;
    let origin = state.config.public_url.clone().unwrap_or_else(|| {
        let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
        format!("https://{host}{base}")
    });
    let url = format!("{}/p/{}{suffix}", origin.trim_end_matches('/'), web::urlencoding(pad_id));
    qr_block(&url, "Mitsehen auf dem Handy:")
}

pub fn qr_block(url: &str, label: &str) -> String {
    let qr = qrcode::QrCode::new(url.as_bytes()).map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(320, 320).quiet_zone(true).build()).unwrap_or_default();
    let qr = qr.split_once("<svg").map(|(_, r)| format!(r#"<svg role="img" aria-label="QR-Code: {}"{r}"#, html_escape::encode_double_quoted_attribute(label.trim_end_matches(':')))).unwrap_or_default();
    format!(r#"{qr}<p>{}<br><strong>{}</strong></p>"#, html_escape::encode_text(label), html_escape::encode_text(url))
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
<section class="quiz-buzz-wahl" id="buzz-wahl" hidden aria-label="Buzzer"></section>
<p class="notice quiz-wach" id="buzz-wach" role="status" hidden>Der Bildschirm bleibt nicht von selbst an. Bitte die automatische Sperre kurz ausschalten (iPhone: Einstellungen › Anzeige &amp; Helligkeit › Automatische Sperre; Android: Einstellungen › Display › Bildschirm-Timeout).</p>
<section class="quiz-board" id="board" aria-label="Raster"></section>
<section class="quiz-question" id="question" hidden aria-live="polite">
<p class="quiz-meta" id="q-meta"></p><p class="quiz-q" id="q-text"></p><p class="quiz-a" id="q-answer" hidden></p>
<button type="button" class="quiz-buzzer" id="buzz-btn" hidden>Buzzer</button>
<p class="hint" id="buzz-info" aria-live="polite"></p>
<p class="quiz-buzz-kopf" id="buzz-kopf" hidden></p><ol class="quiz-buzz-reihe" id="buzz-reihe" hidden aria-live="polite" aria-labelledby="buzz-kopf"></ol>
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
    #[test]
    fn buzzer_reihenfolge_nach_druckzeit_im_fenster() {
        let teams = vec![(1, "Rot".to_string()), (2, "Blau".to_string()), (3, "Grün".to_string())];
        let b = |team, t_press, ts_ein| Buzz { team, t_press, ts_ein, flagged: false };
        // Blau kommt zuerst an, Rot hat früher gedrückt (langsames Netz); Grün 30 ms nach Rot: gleichauf; Rot drückt zweimal
        let v = buzz_order(&[b(2, 1060.0, 1080.0), b(1, 1000.0, 1300.0), b(3, 1030.0, 1200.0), b(1, 1100.0, 1350.0)], &teams, 2000.0);
        let r = v["reihe"].as_array().unwrap();
        assert_eq!(r.iter().map(|x| x["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Rot", "Grün", "Blau"]);
        assert_eq!(r[1]["gleichauf"], json!(true));
        assert_eq!(r[2]["gleichauf"], json!(true));
        assert_eq!(r[2]["abstand"], json!(60.0));
        assert_eq!(v["fertig"], json!(true));
        // nach dem Fenster angekommen: hinten, als spät
        let v = buzz_order(&[b(2, 1060.0, 1080.0), b(1, 900.0, 1700.0)], &teams, 1200.0);
        assert_eq!(v["reihe"][0]["name"], json!("Blau"));
        assert_eq!(v["reihe"][1]["spaet"], json!(true));
        assert_eq!(v["fertig"], json!(false));
    }
    #[test]
    fn buzzer_zeit_plausibel_oder_ersetzt() {
        assert_eq!(buzz_time(500.0, 600.0, 40.0, 1150.0, 900.0), (1100.0, false));
        // Druck angeblich vor dem Öffnen der Frage
        assert_eq!(buzz_time(0.0, 600.0, 40.0, 1150.0, 900.0), (1130.0, true));
        // Druck angeblich nach dem Eingang
        assert!(buzz_time(1000.0, 600.0, 40.0, 1150.0, 900.0).1);
    }

}
