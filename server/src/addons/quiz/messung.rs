//! Buzzer-Messung im Quiz-Raster: misst im Raum, ob ein Buzzer mit Uhrabgleich fair genug wäre. Keine Wertung, keine Namen.
//! Handys gleichen alle 3 s ihre Uhr mit dem Server ab (beste von fünf Anfragen); auf ein Signal der Leitung zeigen sie
//! „Jetzt drücken“. Gespeichert werden je Gerät eine Nummer, die grobe Geräteart und Zeitpunkte, ausgewertet auf dem Server.
use crate::texte::{self, Sprache};
use crate::sperre::Sperre;
use super::daten::{MessMarke, MessProbe};
use crate::{addons::quiz, web, AppState};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

const MAX_GERAETE: i64 = 80;
const MAX_PROBEN: i64 = 20_000;
const MAX_RUNDEN: i64 = 500;
/// So lange nach dem Signal zeigen die Handys „Jetzt drücken“.
const OFFEN_MS: f64 = 10_000.0;
/// Unter diesem Abstand gelten zwei Drücke als gleichzeitig; zugleich die Grenze für das Urteil. Dieselbe wie im Spiel.
const KNAPP_MS: f64 = super::BUZZ_GLEICH_MS;

pub const MESSUNG_JS: &str = include_str!("static/messung.js");
pub const MESSUNG_CSS: &str = include_str!("static/messung.css");

pub(super) fn routen() -> Router<Arc<AppState>> {
    Router::new()
        .route("/p/{id}/messung", get(page))
        .route("/p/{id}/messung/zeit", get(zeit))
        .route("/p/{id}/messung/stand", get(stand))
        .route("/p/{id}/messung/daten", get(daten))
        .route("/p/{id}/messung/export.csv", get(export))
        .route("/p/{id}/messung/probe", post(probe))
        .route("/p/{id}/messung/empfang", post(empfang))
        .route("/p/{id}/messung/druck", post(druck))
        .route("/p/{id}/messung/signal", post(signal))
        .route("/p/{id}/messung/loeschen", post(loeschen))
        .route("/static/messung.js", get(static_js))
        .route("/static/messung.css", get(static_css))
}

pub fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for b in MESSUNG_JS.bytes().chain(MESSUNG_CSS.bytes()) { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        format!("{h:016x}")
    })
}

fn jetzt() -> f64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0) }

fn no_store(mut r: Response) -> Response {
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// Grobe Geräteart aus dem User-Agent; mehr wird über das Gerät nicht gespeichert.
fn art(headers: &HeaderMap) -> &'static str {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    if ua.contains("iPhone") || ua.contains("iPad") { "iOS" } else if ua.contains("Android") { "Android" } else { "andere" }
}

/// Mitmessen darf, wer das Quiz sehen darf, aber nur von der eigenen Seite (Origin).
fn teilnahme(state: &AppState, headers: &HeaderMap, id: &str) -> Result<quiz::Viewer, Response> {
    if !web::origin_ok(state, headers) || headers.get(header::ORIGIN).is_none() { return Err(StatusCode::FORBIDDEN.into_response()); }
    quiz::viewer(state, headers, id)
}

fn zahl(body: &serde_json::Value, k: &str, max: f64) -> Option<f64> {
    body.get(k).and_then(|x| x.as_f64()).filter(|x| x.is_finite() && x.abs() <= max)
}

/// Gerätekennung aus dem Browser: 8 bis 32 Zeichen, nur Buchstaben und Ziffern.
fn geraet(state: &AppState, headers: &HeaderMap, pad: &str, body: &serde_json::Value) -> Result<i64, Response> {
    let dev = body.get("dev").and_then(|x| x.as_str()).unwrap_or("");
    if !(8..=32).contains(&dev.len()) || !dev.chars().all(|c| c.is_ascii_alphanumeric()) { return Err(StatusCode::BAD_REQUEST.into_response()); }
    match state.store.mess_geraet(pad, dev, art(headers), MAX_GERAETE) {
        Ok(Some(nr)) => Ok(nr),
        Ok(None) => Err((StatusCode::TOO_MANY_REQUESTS, texte::tf(Sprache::aus_anfrage(headers), "messung.zu_viele_geraete", &[("n", &MAX_GERAETE.to_string())])).into_response()),
        Err(e) => { tracing::warn!("Messung Gerät: {e}"); Err(StatusCode::INTERNAL_SERVER_ERROR.into_response()) }
    }
}

/// Serverzeit für den Uhrabgleich, so knapp wie möglich.
pub async fn zeit(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    if let Err(r) = quiz::viewer(&state, &headers, &id) { return r; }
    no_store(Json(json!({ "t": jetzt() })).into_response())
}

/// Letzte Runde und ob sie noch offen ist; dazu die Rolle.
pub async fn stand(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match quiz::viewer(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let Ok(runden) = state.store.mess_runden(&v.pad) else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let (runde, ts) = runden.last().copied().unwrap_or((0, 0.0));
    no_store(Json(json!({ "lead": v.lead, "runde": runde, "offen": runde > 0 && jetzt() - ts < OFFEN_MS })).into_response())
}

/// Ergebnis eines Uhrabgleichs: kürzeste Laufzeit, Median der Serie und Versatz der Geräteuhr bei der kürzesten.
pub async fn probe(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match teilnahme(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let (Some(rtt), Some(rtt_med), Some(off)) = (zahl(&body, "rtt", 30_000.0), zahl(&body, "rttMed", 30_000.0), zahl(&body, "off", 1e13))
        else { return StatusCode::BAD_REQUEST.into_response() };
    if rtt < 0.0 || rtt_med < rtt { return StatusCode::BAD_REQUEST.into_response(); }
    let nr = match geraet(&state, &headers, &v.pad, &body) { Ok(n) => n, Err(r) => return r };
    match state.store.mess_probe(&v.pad, &MessProbe { nr, ts: jetzt(), rtt, rtt_med, off }, MAX_PROBEN) {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => web::abgelehnt(&headers, StatusCode::TOO_MANY_REQUESTS, "messung.voll"),
        Err(e) => { tracing::warn!("Messung Abgleich: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

fn marke(state: &AppState, headers: &HeaderMap, id: &str, body: &serde_json::Value, druck: bool) -> Response {
    let v = match teilnahme(state, headers, id) { Ok(v) => v, Err(r) => return r };
    let (Some(runde), Some(t), Some(off), Some(rtt)) = (body.get("runde").and_then(|x| x.as_i64()), zahl(body, "t", 1e14), zahl(body, "off", 1e13), zahl(body, "rtt", 30_000.0))
        else { return StatusCode::BAD_REQUEST.into_response() };
    let nr = match geraet(state, headers, &v.pad, body) { Ok(n) => n, Err(r) => return r };
    let res = if druck { state.store.mess_druck(&v.pad, nr, runde, t, jetzt(), off, rtt) } else { state.store.mess_empfang(&v.pad, nr, runde, t, off, rtt) };
    match res {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => { tracing::warn!("Messung Marke: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn empfang(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    marke(&state, &headers, &id, &body, false)
}

pub async fn druck(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    marke(&state, &headers, &id, &body, true)
}

/// Signal der Leitung: neue Runde, Meldung über den Live-Kanal des Quiz.
pub async fn signal(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match quiz::lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    match state.store.mess_signal(&v.pad, jetzt(), MAX_RUNDEN) {
        Ok(Some(runde)) => { state.live.notify(&v.pad); Json(json!({ "ok": true, "runde": runde })).into_response() }
        Ok(None) => (StatusCode::TOO_MANY_REQUESTS, texte::tf(Sprache::aus_anfrage(&headers), "messung.max_runden", &[("n", &MAX_RUNDEN.to_string())])).into_response(),
        Err(e) => { tracing::warn!("Messung Signal: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn loeschen(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match quiz::lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    match state.store.mess_loeschen(&v.pad) {
        Ok(()) => { tracing::info!("Messdaten gelöscht: Pad {}", v.pad); state.live.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Err(e) => { tracing::warn!("Messung löschen: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

fn nur_leitung(state: &AppState, headers: &HeaderMap, id: &str) -> Result<String, Response> {
    let v = quiz::viewer(state, headers, id)?;
    if !v.lead { return Err((StatusCode::FORBIDDEN, "Die Auswertung sieht nur die Leitung.").into_response()); }
    Ok(v.pad)
}

/// Auswertung für die Leitung.
pub async fn daten(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let pad = match nur_leitung(&state, &headers, &id) { Ok(p) => p, Err(r) => return r };
    if leitung_da(&pad) { state.live.notify(&pad); }
    let (Ok(g), Ok(p), Ok(r), Ok(m)) = (state.store.mess_geraete(&pad), state.store.mess_proben(&pad), state.store.mess_runden(&pad), state.store.mess_marken(&pad))
        else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    no_store(Json(auswerten(&g, &p, &r, &m, jetzt(), Sprache::aus_anfrage(&headers))).into_response())
}

/// Wie lange der Knopf zum Mitmessen nach der letzten Abfrage der Leitung bleibt (die Messseite fragt alle 2 s).
const LEITUNG_DA_MS: f64 = 10_000.0;

/// Wann die Leitung je Quiz zuletzt die Messseite offen hatte (ms); solange zeigt das Quiz den Knopf zum Mitmessen.
/// Nur im Speicher, nach einem Neustart fragt die Messseite binnen 2 s wieder.
fn offen() -> &'static std::sync::Mutex<std::collections::HashMap<String, f64>> {
    static O: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, f64>>> = std::sync::OnceLock::new();
    O.get_or_init(Default::default)
}

/// Merkt, dass die Leitung die Messseite offen hat; true, wenn sie eben erst dazukam (dann sollen die Handys neu laden).
fn leitung_da(pad: &str) -> bool {
    let now = jetzt();
    let mut m = offen().sperren();
    m.retain(|_, t| now - *t < 3_600_000.0);
    let neu = m.get(pad).is_none_or(|t| now - *t >= LEITUNG_DA_MS);
    m.insert(pad.to_string(), now);
    neu
}

/// Ob die Leitung die Messseite gerade offen hat.
pub fn leitung_misst(pad: &str) -> bool {
    let m = offen().sperren();
    m.get(pad).is_some_and(|t| jetzt() - *t < LEITUNG_DA_MS)
}

/// Rohdaten als CSV zum Nachrechnen: eine Zeile je Abgleich und je Gerät und Runde. Zeiten in ms, Serverzeit ab Messbeginn.
pub async fn export(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let pad = match nur_leitung(&state, &headers, &id) { Ok(p) => p, Err(r) => return r };
    let (Ok(g), Ok(p), Ok(r), Ok(m)) = (state.store.mess_geraete(&pad), state.store.mess_proben(&pad), state.store.mess_runden(&pad), state.store.mess_marken(&pad))
        else { return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let arten: BTreeMap<i64, &str> = g.iter().map(|(n, a)| (*n, a.as_str())).collect();
    let t0 = p.iter().map(|x| x.ts).chain(r.iter().map(|x| x.1)).fold(f64::INFINITY, f64::min);
    let t0 = if t0.is_finite() { t0 } else { 0.0 };
    let f = |x: Option<f64>| x.map(|v| format!("{v:.1}")).unwrap_or_default();
    let mut out = String::from("zeile,geraet,geraeteart,runde,zeit_ms,laufzeit_ms,laufzeit_median_ms,versatz_ms,signal_verzug_ms,reaktion_ms,ankunft_verzug_ms\n");
    for x in &p {
        out.push_str(&format!("abgleich,{},{},,{:.1},{:.1},{:.1},{:.1},,,\n", x.nr, arten.get(&x.nr).unwrap_or(&""), x.ts - t0, x.rtt, x.rtt_med, x.off));
    }
    let runden: BTreeMap<i64, f64> = r.iter().copied().collect();
    for x in &m {
        let ts = runden.get(&x.runde).copied().unwrap_or(t0);
        let k = Kennzahlen::von(x, ts);
        out.push_str(&format!("runde,{},{},{},{:.1},{:.1},,{:.1},{},{},{}\n", x.nr, arten.get(&x.nr).unwrap_or(&""), x.runde, ts - t0, x.rtt, x.off, f(k.signal), f(k.reaktion), f(k.verzug)));
    }
    let dispo = format!("attachment; filename=\"buzzer-messung-{}.csv\"", chrono_tag());
    let mut resp = ([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], out).into_response();
    if let Ok(v) = HeaderValue::from_str(&dispo) { resp.headers_mut().insert(header::CONTENT_DISPOSITION, v); }
    no_store(resp)
}

fn chrono_tag() -> String {
    let tage = (jetzt() / 86_400_000.0) as i64;
    // Tage seit 1970 in ein Datum (proleptischer gregorianischer Kalender, nach Howard Hinnant)
    let z = tage + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Abgeleitete Zeiten eines Geräts in einer Runde.
struct Kennzahlen {
    /// Signal kommt auf dem Gerät an, gemessen ab dem Senden (Serverzeit).
    signal: Option<f64>,
    /// Druck minus Signal, beides auf der Geräteuhr; braucht keinen Abgleich.
    reaktion: Option<f64>,
    /// Eingang beim Server minus Druck in Serverzeit: so viel verschiebt das Netz den Druck.
    verzug: Option<f64>,
    /// Druck in Serverzeit (nach Abgleich).
    druck: Option<f64>,
    ein: Option<f64>,
}

impl Kennzahlen {
    fn von(m: &MessMarke, ts_runde: f64) -> Self {
        let druck = m.t_druck.map(|t| t + m.off);
        Kennzahlen {
            signal: m.t_empfang.map(|t| t + m.off - ts_runde),
            reaktion: m.t_druck.zip(m.t_empfang).map(|(d, e)| d - e),
            verzug: druck.zip(m.ts_ein).map(|(d, e)| e - d),
            druck,
            ein: m.ts_ein,
        }
    }
}

/// Perzentil (nächster Rang), auf 0,1 ms gerundet; None ohne Werte.
fn pct(v: &[f64], p: f64) -> Option<f64> {
    if v.is_empty() { return None; }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let i = ((p / 100.0) * (s.len() - 1) as f64).round() as usize;
    Some((s[i] * 10.0).round() / 10.0)
}

/// Abstand zwischen größtem und kleinstem Wert, erst ab zwei Werten.
fn spanne(v: &[f64]) -> Option<f64> {
    let max = v.iter().copied().reduce(f64::max)?;
    let min = v.iter().copied().reduce(f64::min)?;
    (v.len() >= 2).then_some(max - min)
}

fn erster(v: &[(i64, f64)]) -> Option<i64> { v.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0) }

/// Kern der Messung. Uhr-Unsicherheit: Sprung des Versatzes zwischen zwei Abgleichen desselben Geräts im Abstand unter 15 s
/// (die Drift der Geräteuhr ist dort vernachlässigbar). Das ist der Fehler, mit dem ein Buzzer Klickzeiten vergleichen müsste.
pub fn auswerten(geraete: &[(i64, String)], proben: &[MessProbe], runden: &[(i64, f64)], marken: &[MessMarke], jetzt: f64, sp: Sprache) -> serde_json::Value {
    let rtts: Vec<f64> = proben.iter().map(|p| p.rtt).collect();
    let mut je_geraet: BTreeMap<i64, Vec<&MessProbe>> = BTreeMap::new();
    for p in proben { je_geraet.entry(p.nr).or_default().push(p); }
    let mut spruenge = Vec::new();
    let mut zeilen = Vec::new();
    let mut aktiv = 0;
    for (nr, art) in geraete {
        let mut ps = je_geraet.get(nr).cloned().unwrap_or_default();
        ps.sort_by(|a, b| a.ts.total_cmp(&b.ts));
        let eigene: Vec<f64> = ps.windows(2).filter(|w| w[1].ts - w[0].ts < 15_000.0).map(|w| (w[1].off - w[0].off).abs()).collect();
        let zuletzt = ps.last().map(|p| jetzt - p.ts);
        if zuletzt.is_some_and(|z| z < 10_000.0) { aktiv += 1; }
        let druecke = marken.iter().filter(|m| m.nr == *nr && m.t_druck.is_some()).count();
        zeilen.push(json!({
            "nr": nr, "art": art, "abgleiche": ps.len(),
            "laufzeit": pct(&ps.iter().map(|p| p.rtt).collect::<Vec<_>>(), 50.0),
            "uhr": pct(&eigene, 95.0),
            "zuletztS": zuletzt.map(|z| (z / 1000.0).round()),
            "druecke": druecke,
        }));
        spruenge.extend(eigene);
    }

    let (mut signale, mut verzuege, mut reaktionen) = (Vec::new(), Vec::new(), Vec::new());
    let (mut signal_spannen, mut verzug_spannen) = (Vec::new(), Vec::new());
    let (mut gewertet, mut wechsel_eingang, mut wechsel_reaktion, mut knapp) = (0, 0, 0, 0);
    for (runde, ts) in runden {
        let k: Vec<(i64, Kennzahlen)> = marken.iter().filter(|m| m.runde == *runde).map(|m| (m.nr, Kennzahlen::von(m, *ts))).collect();
        let sig: Vec<f64> = k.iter().filter_map(|(_, x)| x.signal).collect();
        let ver: Vec<f64> = k.iter().filter_map(|(_, x)| x.verzug).collect();
        if let Some(s) = spanne(&sig) { signal_spannen.push(s); }
        if let Some(s) = spanne(&ver) { verzug_spannen.push(s); }
        signale.extend(&sig);
        verzuege.extend(&ver);
        reaktionen.extend(k.iter().filter_map(|(_, x)| x.reaktion));
        let gedrueckt: Vec<&(i64, Kennzahlen)> = k.iter().filter(|(_, x)| x.druck.is_some() && x.ein.is_some()).collect();
        if gedrueckt.len() < 2 { continue; }
        gewertet += 1;
        let nach_druck: Vec<(i64, f64)> = gedrueckt.iter().map(|(n, x)| (*n, x.druck.unwrap_or_default())).collect();
        let nach_eingang: Vec<(i64, f64)> = gedrueckt.iter().map(|(n, x)| (*n, x.ein.unwrap_or_default())).collect();
        if erster(&nach_druck) != erster(&nach_eingang) { wechsel_eingang += 1; }
        let nach_reaktion: Vec<(i64, f64)> = gedrueckt.iter().filter_map(|(n, x)| x.reaktion.map(|r| (*n, r))).collect();
        if nach_reaktion.len() == gedrueckt.len() && erster(&nach_reaktion) != erster(&nach_druck) { wechsel_reaktion += 1; }
        let mut d: Vec<f64> = nach_druck.iter().map(|x| x.1).collect();
        d.sort_by(|a, b| a.total_cmp(b));
        if d[1] - d[0] < KNAPP_MS { knapp += 1; }
    }

    let uhr95 = pct(&spruenge, 95.0);
    let urteil = match uhr95 {
        _ if spruenge.len() < 20 || geraete.len() < 3 => texte::t(sp, "messung.u.wenig").to_string(),
        Some(u) => texte::tf(sp, if u < KNAPP_MS { "messung.u.traegt" } else { "messung.u.reicht_nicht" }, &[("u", &format!("{u:.0}")), ("grenze", &format!("{KNAPP_MS:.0}"))]),
        None => String::new(),
    };
    json!({
        "geraete": geraete.len(), "aktiv": aktiv, "abgleiche": proben.len(), "runden": runden.len(),
        "laufzeit": { "median": pct(&rtts, 50.0), "p95": pct(&rtts, 95.0), "ueber300": if rtts.is_empty() { 0.0 } else { (rtts.iter().filter(|r| **r > 300.0).count() as f64 * 1000.0 / rtts.len() as f64).round() / 10.0 } },
        "uhr": { "median": pct(&spruenge, 50.0), "p95": uhr95 },
        "signal": { "median": pct(&signale, 50.0), "p95": pct(&signale, 95.0), "spanneMedian": pct(&signal_spannen, 50.0), "spanneMax": pct(&signal_spannen, 100.0) },
        "eingang": { "median": pct(&verzuege, 50.0), "p95": pct(&verzuege, 95.0), "spanneMedian": pct(&verzug_spannen, 50.0), "spanneMax": pct(&verzug_spannen, 100.0) },
        "reaktion": { "median": pct(&reaktionen, 50.0) },
        "vergleich": { "runden": gewertet, "wechselEingang": wechsel_eingang, "wechselReaktion": wechsel_reaktion, "knapp": knapp },
        "urteil": urteil,
        "liste": zeilen,
    })
}

/// Eine Seite für Leitung und Handys; das Skript zeigt je nach Rolle.
pub async fn page(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let access = match web::check_access(&state, &headers, &id) { Ok(a) => a, Err((s, m)) => return web::abgelehnt(&headers, s, m) };
    if state.store.kind(&access.pad.id).as_deref() != Some(quiz::ADDON.art) { return web::abgelehnt(&headers, StatusCode::NOT_FOUND, "quiz.kein_quiz"); }
    let base = &state.config.base_path;
    let join = quiz::join_svg(&state, &headers, access.free, &id, &access.pad.id, "/messung");
    let sp = Sprache::aus_anfrage(&headers);
    let t = |k| texte::t(sp, k);
    let cfg = json!({ "urlId": id, "basePath": base, "lang": sp.kuerzel(), "texte": texte::texte_mit(sp, "messungjs.") });
    let content = format!(r#"<main id="inhalt" class="messung" tabindex="-1">
<p class="kicker">{kicker}</p>
<h1>{titel}</h1>
<section id="m-handy" hidden>
<p>{handy}</p>
<p><button type="button" class="btn btn-primary" id="m-start">{mitmessen}</button></p>
<p class="hint" id="m-status" aria-live="polite">{offen_lassen}</p>
<p class="notice" id="m-wach" role="status" hidden>{wach}</p>
<button type="button" class="messung-buzzer" id="m-buzzer" disabled>{warten}</button>
<p class="messung-reaktion" id="m-reaktion" aria-live="polite"></p>
</section>
<section id="m-lead" hidden>
<div class="messung-kopf">
<aside class="panel quiz-qr" id="qr-box">{join}</aside>
<div class="messung-steuerung">
<p class="messung-zahl">{aktiv}</p>
<p><button type="button" class="btn btn-primary" id="m-signal">{signal}</button> <span class="hint">{leertaste}</span></p>
<p class="hint">{ablauf}</p>
</div></div>
<p class="messung-urteil" id="m-urteil" aria-live="polite"></p>
<dl class="messung-werte" id="m-werte"></dl>
<p class="hint">{hinweis}</p>
<details><summary>{geraete}</summary><table class="messung-liste"><thead><tr><th scope="col">{geraet}</th><th scope="col">{art}</th><th scope="col">{abgleiche}</th><th scope="col">{laufzeit}</th><th scope="col">{uhr}</th><th scope="col">{druecke}</th><th scope="col">{zuletzt}</th></tr></thead><tbody id="m-liste"></tbody></table></details>
<p class="messung-aktionen"><a class="btn btn-secondary" href="{base}/p/{u}/messung/export.csv">{csv}</a> <button type="button" class="btn btn-secondary" id="m-loeschen">{loeschen}</button> <a class="btn-link" href="{base}/p/{u}">{zum_quiz}</a></p>
</section>
<noscript><p class="notice">{braucht_js}</p></noscript>
</main>
<script type="application/json" id="nextpad-config">{cfg}</script>"#, u = web::urlencoding(&id), cfg = cfg.to_string().replace("</", "<\\/"),
        kicker = t("messung.s.kicker"), titel = t("messung.s.titel"), handy = t("messung.s.handy"), mitmessen = t("messung.s.mitmessen"), offen_lassen = t("messung.s.offen_lassen"),
        wach = t("quiz.s.wach_html"), warten = t("messung.s.warten"), signal = t("messung.s.signal"), leertaste = t("messung.s.leertaste"), ablauf = t("messung.s.ablauf"),
        aktiv = texte::tf(sp, "messung.s.aktiv_html", &[("zahl", r#"<strong id="m-aktiv">0</strong>"#)]),
        hinweis = t("messung.s.hinweis"), geraete = t("messung.s.geraete"), geraet = t("messung.s.geraet"), art = t("messung.s.art"), abgleiche = t("messung.s.abgleiche"),
        laufzeit = t("messung.s.laufzeit"), uhr = t("messung.s.uhr"), druecke = t("messung.s.druecke"), zuletzt = t("messung.s.zuletzt"), csv = t("messung.s.csv"),
        loeschen = t("messung.s.loeschen"), zum_quiz = t("messung.s.zum_quiz"), braucht_js = t("messung.s.braucht_js"));
    let head = format!(r#"<link rel="stylesheet" href="{base}/static/quiz.css?v={q}"><link rel="stylesheet" href="{base}/static/messung.css?v={v}">"#, q = quiz::version(), v = version());
    let scripts = format!(r#"<script src="{base}/static/messung.js?v={v}"></script>"#, v = version());
    let mut resp = Html(web::page_frame(&state, sp, Some(&format!("/p/{}/messung", web::urlencoding(&id))), t("messung.s.seitentitel"), &head, &content, &scripts)).into_response();
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors {}; base-uri 'none'", state.config.frame_ancestors);
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    no_store(resp)
}

pub async fn static_js() -> Response { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], MESSUNG_JS).into_response() }
pub async fn static_css() -> Response { ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], MESSUNG_CSS).into_response() }

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(nr: i64, ts: f64, off: f64) -> MessProbe { MessProbe { nr, ts, rtt: 40.0, rtt_med: 60.0, off } }

    #[test]
    fn uhr_unsicherheit_aus_spruengen() {
        let g: Vec<(i64, String)> = (1..=3).map(|n| (n, "iOS".to_string())).collect();
        // je Gerät 10 Abgleiche im Abstand 3 s, Versatz springt abwechselnd um 0 und 8 ms
        let p: Vec<MessProbe> = (1..=3).flat_map(|n| (0..10).map(move |i| probe(n, i as f64 * 3000.0, 1000.0 * n as f64 + if i % 2 == 0 { 0.0 } else { 8.0 }))).collect();
        let a = auswerten(&g, &p, &[], &[], 30_000.0, Sprache::De);
        assert_eq!(a["uhr"]["p95"], json!(8.0));
        assert!(a["urteil"].as_str().unwrap().starts_with("Der Uhrabgleich trägt"));
        assert!(auswerten(&g, &p, &[], &[], 30_000.0, Sprache::En)["urteil"].as_str().unwrap().starts_with("The clock sync holds"));
        assert_eq!(a["aktiv"], json!(3));
    }

    #[test]
    fn grosse_pause_zaehlt_nicht_als_sprung() {
        let g = vec![(1, "Android".to_string())];
        let p = vec![probe(1, 0.0, 0.0), probe(1, 60_000.0, 500.0)];
        let a = auswerten(&g, &p, &[], &[], 61_000.0, Sprache::De);
        assert_eq!(a["uhr"]["p95"], serde_json::Value::Null);
    }

    #[test]
    fn spanne_erst_ab_zwei_werten() {
        assert_eq!(spanne(&[]), None);
        assert_eq!(spanne(&[5.0]), None);
        assert_eq!(spanne(&[-20.0, 3.0, -4.5]), Some(23.0));
    }

    #[test]
    fn eingang_und_druckzeit_koennen_sich_widersprechen() {
        // Gerät 1 drückt früher (Serverzeit 1100), kommt aber später an als Gerät 2 (drückt 1150, schnelles Netz).
        let m = vec![
            MessMarke { nr: 1, runde: 1, t_empfang: Some(1050.0), t_druck: Some(1100.0), ts_ein: Some(1400.0), off: 0.0, rtt: 300.0 },
            MessMarke { nr: 2, runde: 1, t_empfang: Some(5020.0), t_druck: Some(5150.0), ts_ein: Some(1170.0), off: -4000.0, rtt: 30.0 },
        ];
        let a = auswerten(&[], &[], &[(1, 1000.0)], &m, 2000.0, Sprache::De);
        assert_eq!(a["vergleich"]["runden"], json!(1));
        assert_eq!(a["vergleich"]["wechselEingang"], json!(1));
        // Reaktion: Gerät 1 50 ms, Gerät 2 130 ms → gleicher Erster wie nach Druckzeit
        assert_eq!(a["vergleich"]["wechselReaktion"], json!(0));
        assert_eq!(a["vergleich"]["knapp"], json!(0));
        assert_eq!(a["signal"]["spanneMax"], json!(30.0));
        assert_eq!(a["eingang"]["p95"], json!(300.0));
    }
}
