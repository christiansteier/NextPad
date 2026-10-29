//! Ideensammlung: Ideen und Fragen sammeln und abstimmen (nach dem Vorbild von liveask).
//! Der Server prüft jede Stimme: eine je Person und Beitrag, moderieren darf nur, wer die Sammlung angelegt hat.
//! Beiträge erscheinen ohne Namen. Änderungen gehen per Server-Sent Events an alle offenen Seiten.
use super::{Addon, Seite};
use crate::{texte::{self, Sprache}, web, AppState};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::broadcast;

pub mod daten;

pub const MAX_TEXT: usize = 500;
const MAX_PROMPT: usize = 200;
const MAX_TOTAL: i64 = 500;
const MAX_OWN: i64 = 30;

pub const IDEEN_JS: &str = include_str!("static/ideen.js");
pub const IDEEN_CSS: &str = include_str!("static/ideen.css");
pub const WAND_JS: &str = include_str!("static/ideen-wand.js");

const TEXTE_DE: &[(&str, &str)] = &[
    ("ideen.sichern", "Die Beiträge lassen sich in der Sammlung unter „Teilen, zeigen, sichern“ als Markdown oder CSV sichern."),
    ("ideen.x.titel", "Ideensammlung"),
    ("ideen.x.erledigt", " (erledigt)"),
    ("ideen.x.stimme", "Stimme"),
    ("ideen.x.stimmen", "Stimmen"),
    ("ideen.x.kopf", "Nr;Beitrag;Stimmen;Erledigt"),
    ("ideen.x.ja", "ja"),
    ("ideen.x.nein", "nein"),
    ("ideen.x.datei", "ideen.csv"),
    ("ideen.s.kicker", "Ideensammlung · Bearbeitungsansicht"),
    ("ideen.s.titel", "Ideen und Fragen"),
    ("ideen.s.frage_aendern", "Frage ändern"),
    ("ideen.s.frage_label", "Frage oder Thema der Sammlung"),
    ("ideen.s.speichern", "Speichern"),
    ("ideen.s.idee_label", "Deine Idee oder Frage"),
    ("ideen.s.zeichen_html", "Ohne Namen. {zaehler} von {max} Zeichen."),
    ("ideen.s.abschicken", "Abschicken"),
    ("ideen.s.braucht_js", "Die Ideensammlung braucht JavaScript."),
    ("ideen.s.sortieren", "Sortieren"),
    ("ideen.s.beliebt", "Beliebt"),
    ("ideen.s.neu", "Neu"),
    ("ideen.s.offene", "Offene Beiträge"),
    ("ideen.s.leer_teilen", "Noch keine Beiträge. Den Link teilen, dann können alle mitmachen."),
    ("ideen.s.erledigt", "Erledigt"),
    ("ideen.s.ausgeblendet", "Ausgeblendet"),
    ("ideen.s.nur_fuer_dich", "Nur für dich sichtbar."),
    ("ideen.s.teilen", "Teilen, zeigen, sichern"),
    ("ideen.s.link_kopieren", "Link zum Mitmachen kopieren"),
    ("ideen.s.link_hinweis", "Wer den Link hat, kann Beiträge eintragen und abstimmen. Ohne Anmeldung und ohne Namen."),
    ("ideen.s.leinwand_oeffnen", "Leinwand öffnen"),
    ("ideen.s.leinwand_hinweis", "Große Ansicht für den Beamer mit den beliebtesten Beiträgen und einem QR-Code zum Mitmachen. Sie aktualisiert sich selbst."),
    ("ideen.s.sichern", "Sichern:"),
    ("ideen.s.csv", "CSV für Tabellen"),
    ("ideen.s.loeschen", "Sammlung löschen"),
    ("ideen.s.seitentitel", "NextPad · Ideensammlung"),
    ("ideen.s.leinwand_titel", "Leinwand · NextPad"),
    ("ideen.s.leer", "Noch keine Beiträge."),
    ("ideen.s.qr", "QR-Code zum Mitmachen"),
    ("ideen.s.mitmachen", "Mitmachen:"),
    ("ideen.s.mitmachen_moodle", "Mitmachen über Moodle:"),
    ("ideen.s.mitmachen_kurs", "Mitmachen über die Aktivität im Moodle-Kurs."),
    ("ideen.keine_sammlung", "Das ist keine Ideensammlung."),
    ("ideen.nur_mitlesen", "Über diesen Link lässt sich nur mitlesen."),
    ("ideen.text_fehlt", "Bitte einen Text eingeben."),
    ("ideen.voll", "Die Sammlung ist voll oder du hast schon sehr viele Beiträge geschrieben."),
    ("ideen.abstimmen_zu", "Über diesen Beitrag lässt sich nicht mehr abstimmen."),
    ("ideen.nur_besitz", "Nur wer die Sammlung angelegt hat, darf das."),
    ("ideenjs.zum_kurs", "Zurück zum Kurs"),
    ("ideenjs.fehler", "Fehler {status}"),
    ("ideenjs.stimme", "{n} Stimme"),
    ("ideenjs.stimmen", "{n} Stimmen"),
    ("ideenjs.zuruecknehmen", "Stimme zurücknehmen"),
    ("ideenjs.zustimmen", "Zustimmen"),
    ("ideenjs.erledigt", "erledigt"),
    ("ideenjs.dein", "Dein Beitrag"),
    ("ideenjs.wieder_oeffnen", "Wieder öffnen"),
    ("ideenjs.als_erledigt", "Als erledigt markieren"),
    ("ideenjs.einblenden", "Wieder einblenden"),
    ("ideenjs.ausblenden", "Ausblenden"),
    ("ideenjs.titel", "Ideen und Fragen"),
    ("ideenjs.name", "Ideensammlung"),
    ("ideenjs.keine_offenen", "Keine offenen Beiträge."),
    ("ideenjs.leer_teilen", "Noch keine Beiträge. Den Link teilen, dann können alle mitmachen."),
    ("ideenjs.leer", "Noch keine Beiträge."),
    ("ideenjs.offen1", "{n} offener Beitrag"),
    ("ideenjs.offen", "{n} offene Beiträge"),
    ("ideenjs.weg", "Diese Sammlung gibt es nicht mehr."),
    ("ideenjs.eingeben", "Bitte eine Idee oder Frage eingeben."),
    ("ideenjs.leseansicht", "Ideensammlung · Leseansicht"),
    ("ideenjs.zeigen_sichern", "Zeigen und sichern"),
    ("ideenjs.nur_moodle", "Zugang nur über die Aktivität im Moodle-Kurs."),
    ("ideenjs.kopiert", "Link kopiert"),
    ("ideenjs.link_kopieren", "Link zum Mitmachen kopieren"),
    ("ideenjs.loeschen_frage", "Die Sammlung samt allen Beiträgen und Stimmen löschen? Das lässt sich nicht rückgängig machen."),
    ("ideenjs.wand_titel", "{titel} · Leinwand"),
    ("ideenjs.beliebteste", ", hier die {n} beliebtesten"),
    ("ideen.name", "Ideensammlung"),
    ("ideen.kachel_html", r#"<li><a class="template" data-tint="yellow" href="{base}/new?typ=ideen"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3z"/></svg><span class="template-name">Ideensammlung</span><span class="template-desc">Ideen und Fragen sammeln, abstimmen</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>"#),
    ("ideen.hilfe_html", r#"<h2>Ideensammlung</h2><p>„Ideensammlung“ legt eine Liste an, in die alle ohne Namen Ideen oder Fragen eintragen und für Beiträge stimmen. Jede Person hat je Beitrag eine Stimme. Wer die Sammlung angelegt hat, kann oben die Frage setzen, Beiträge als erledigt markieren oder ausblenden. „Link zum Mitmachen kopieren“ holt die Adresse für die Gruppe. „Leinwand öffnen“ zeigt die beliebtesten Beiträge groß für den Beamer, bei freien Sammlungen mit QR-Code zum Mitmachen. Gesichert wird als Markdown oder CSV. Im Moodle-Kurs gibt es die Ideensammlung auch als Aktivität: Dann kommen nur Kursmitglieder hinein, und Lehrende moderieren.</p>"#),
    ("ideen.export_formate", "Ideensammlungen lassen sich als Markdown, Text oder CSV sichern."),
];
const TEXTE_EN: &[(&str, &str)] = &[
    ("ideen.sichern", "The entries can be saved as Markdown or CSV in the collection under “Share, show, save”."),
    ("ideen.x.titel", "Idea collection"),
    ("ideen.x.erledigt", " (done)"),
    ("ideen.x.stimme", "vote"),
    ("ideen.x.stimmen", "votes"),
    ("ideen.x.kopf", "No,Entry,Votes,Done"),
    ("ideen.x.ja", "yes"),
    ("ideen.x.nein", "no"),
    ("ideen.x.datei", "ideas.csv"),
    ("ideen.s.kicker", "Idea collection · Editing view"),
    ("ideen.s.titel", "Ideas and questions"),
    ("ideen.s.frage_aendern", "Change question"),
    ("ideen.s.frage_label", "Question or topic of the collection"),
    ("ideen.s.speichern", "Save"),
    ("ideen.s.idee_label", "Your idea or question"),
    ("ideen.s.zeichen_html", "No name. {zaehler} of {max} characters."),
    ("ideen.s.abschicken", "Submit"),
    ("ideen.s.braucht_js", "The idea collection needs JavaScript."),
    ("ideen.s.sortieren", "Sort"),
    ("ideen.s.beliebt", "Popular"),
    ("ideen.s.neu", "New"),
    ("ideen.s.offene", "Open entries"),
    ("ideen.s.leer_teilen", "No entries yet. Share the link so everyone can take part."),
    ("ideen.s.erledigt", "Done"),
    ("ideen.s.ausgeblendet", "Hidden"),
    ("ideen.s.nur_fuer_dich", "Visible only to you."),
    ("ideen.s.teilen", "Share, show, save"),
    ("ideen.s.link_kopieren", "Copy link to take part"),
    ("ideen.s.link_hinweis", "Anyone with the link can add entries and vote. No sign-in and no names."),
    ("ideen.s.leinwand_oeffnen", "Open board"),
    ("ideen.s.leinwand_hinweis", "Large view for the projector with the most popular entries and a QR code to take part. It updates itself."),
    ("ideen.s.sichern", "Save:"),
    ("ideen.s.csv", "CSV for spreadsheets"),
    ("ideen.s.loeschen", "Delete collection"),
    ("ideen.s.seitentitel", "NextPad · Idea collection"),
    ("ideen.s.leinwand_titel", "Board · NextPad"),
    ("ideen.s.leer", "No entries yet."),
    ("ideen.s.qr", "QR code to take part"),
    ("ideen.s.mitmachen", "Take part:"),
    ("ideen.s.mitmachen_moodle", "Take part through Moodle:"),
    ("ideen.s.mitmachen_kurs", "Take part through the activity in the Moodle course."),
    ("ideen.keine_sammlung", "This is not an idea collection."),
    ("ideen.nur_mitlesen", "This link is for reading only."),
    ("ideen.text_fehlt", "Please enter a text."),
    ("ideen.voll", "The collection is full, or you have already written a great many entries."),
    ("ideen.abstimmen_zu", "Voting on this entry is no longer possible."),
    ("ideen.nur_besitz", "Only the person who created the collection can do this."),
    ("ideenjs.zum_kurs", "Back to course"),
    ("ideenjs.fehler", "Error {status}"),
    ("ideenjs.stimme", "{n} vote"),
    ("ideenjs.stimmen", "{n} votes"),
    ("ideenjs.zuruecknehmen", "Take back vote"),
    ("ideenjs.zustimmen", "Agree"),
    ("ideenjs.erledigt", "done"),
    ("ideenjs.dein", "Your entry"),
    ("ideenjs.wieder_oeffnen", "Reopen"),
    ("ideenjs.als_erledigt", "Mark as done"),
    ("ideenjs.einblenden", "Show again"),
    ("ideenjs.ausblenden", "Hide"),
    ("ideenjs.titel", "Ideas and questions"),
    ("ideenjs.name", "Idea collection"),
    ("ideenjs.keine_offenen", "No open entries."),
    ("ideenjs.leer_teilen", "No entries yet. Share the link so everyone can take part."),
    ("ideenjs.leer", "No entries yet."),
    ("ideenjs.offen1", "{n} open entry"),
    ("ideenjs.offen", "{n} open entries"),
    ("ideenjs.weg", "This collection no longer exists."),
    ("ideenjs.eingeben", "Please enter an idea or question."),
    ("ideenjs.leseansicht", "Idea collection · Reading view"),
    ("ideenjs.zeigen_sichern", "Show and save"),
    ("ideenjs.nur_moodle", "Access only through the activity in the Moodle course."),
    ("ideenjs.kopiert", "Link copied"),
    ("ideenjs.link_kopieren", "Copy link to take part"),
    ("ideenjs.loeschen_frage", "Delete the collection with all entries and votes? This cannot be undone."),
    ("ideenjs.wand_titel", "{titel} · Board"),
    ("ideenjs.beliebteste", ", showing the {n} most popular"),
    ("ideen.name", "Idea collection"),
    ("ideen.kachel_html", r#"<li><a class="template" data-tint="yellow" href="{base}/new?typ=ideen"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3z"/></svg><span class="template-name">Idea collection</span><span class="template-desc">Collect ideas and questions, vote</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>"#),
    ("ideen.hilfe_html", r#"<h2>Idea collection</h2><p>“Idea collection” creates a list where everyone adds ideas or questions without a name and votes for entries. Each person has one vote per entry. The person who created the collection can set the question at the top, mark entries as done or hide them. “Copy link to take part” copies the address for the group. “Open board” shows the most popular entries large for the projector, for free collections with a QR code to take part. The collection can be saved as Markdown or CSV. In a Moodle course, the idea collection is also available as an activity: then only course members can take part, and teachers moderate.</p>"#),
    ("ideen.export_formate", "Idea collections can be saved as Markdown, text or CSV."),
];

pub const ADDON: Addon = Addon {
    kennung: "ideen",
    art: "ideas",
    name: "ideen.name",
    daten: Some(&daten::ZUSATZ),
    routen,
    seite,
    export: export_antwort,
    kachel: "ideen.kachel_html",
    sichern: "ideen.sichern",
    hilfe: "ideen.hilfe_html",
    moodle: Some(|store, pad, titel| store.set_idea_prompt(pad, titel)),
    texte: (TEXTE_DE, TEXTE_EN),
};

fn routen() -> Router<Arc<AppState>> {
    Router::new()
        .route("/p/{id}/ideen", get(get_).post(add))
        .route("/p/{id}/ideen/live", get(live))
        .route("/p/{id}/ideen/frage", post(prompt))
        .route("/p/{id}/ideen/{n}/stimme", post(vote))
        .route("/p/{id}/ideen/{n}/moderation", post(moderate))
        .route("/p/{id}/leinwand", get(wall))
        .route("/static/ideen.js", get(static_js))
        .route("/static/ideen-wand.js", get(static_wand))
        .route("/static/ideen.css", get(static_css))
}

fn seite(s: Seite) -> Response {
    let sp = Sprache::aus_anfrage(s.headers);
    let mut cfg = s.cfg;
    cfg["readonlyId"] = json!(s.readonly_id);
    cfg["lang"] = json!(sp.kuerzel());
    cfg["texte"] = texte::texte_mit(sp, "ideenjs.");
    page(s.state, sp, s.url_id, cfg)
}

fn export_antwort(state: &AppState, pad: &str, format: &str, sp: Sprache) -> Response {
    export(state, pad, format, sp).unwrap_or_else(|| web::error_page(sp, StatusCode::NOT_FOUND, texte::t(sp, "ideen.export_formate")))
}

pub fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for b in IDEEN_JS.bytes().chain(IDEEN_CSS.bytes()).chain(WAND_JS.bytes()) { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
        format!("{h:016x}")
    })
}

/// `owner`: darf moderieren (wer die freie Sammlung angelegt hat, in Kursen die Lehrenden);
/// `can_delete`: darf die ganze Sammlung löschen (nur freie Sammlungen, Kursaktivitäten verwaltet Moodle).
struct Viewer { pad: String, author: Option<String>, readonly: bool, owner: bool, can_delete: bool }

fn viewer(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Viewer, Response> {
    let access = web::check_access(state, headers, id).map_err(|(s, m)| web::abgelehnt(headers, s, m))?;
    if state.store.kind(&access.pad.id).as_deref() != Some(ADDON.art) { return Err(web::abgelehnt(headers, StatusCode::NOT_FOUND, "ideen.keine_sammlung")); }
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
        _ => Err(web::abgelehnt(headers, StatusCode::FORBIDDEN, "ideen.nur_mitlesen")),
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
pub async fn get_(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
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
    if text.is_empty() { return web::abgelehnt(&headers, StatusCode::BAD_REQUEST, "ideen.text_fehlt"); }
    match state.store.add_idea(&v.pad, &author, &text, MAX_TOTAL, MAX_OWN) {
        Ok(Some(n)) => { state.live.notify(&v.pad); Json(json!({ "ok": true, "id": n })).into_response() }
        Ok(None) => web::abgelehnt(&headers, StatusCode::TOO_MANY_REQUESTS, "ideen.voll"),
        Err(e) => { tracing::warn!("Idee speichern: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn vote(State(state): State<Arc<AppState>>, Path((id, n)): Path<(String, i64)>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, author) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    let on = body.get("on").and_then(|b| b.as_bool()).unwrap_or(true);
    match state.store.vote(&v.pad, n, &author, on) {
        Ok(true) => { state.live.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => web::abgelehnt(&headers, StatusCode::CONFLICT, "ideen.abstimmen_zu"),
        Err(e) => { tracing::warn!("Stimme: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn moderate(State(state): State<Arc<AppState>>, Path((id, n)): Path<(String, i64)>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, _) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    if !v.owner { return web::abgelehnt(&headers, StatusCode::FORBIDDEN, "ideen.nur_besitz"); }
    let flag = |k: &str| body.get(k).and_then(|b| b.as_bool());
    match state.store.moderate_idea(&v.pad, n, flag("hidden"), flag("done")) {
        Ok(true) => { state.live.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => { tracing::warn!("Moderation: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn prompt(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let (v, _) = match writer(&state, &headers, &id) { Ok(x) => x, Err(r) => return r };
    if !v.owner { return web::abgelehnt(&headers, StatusCode::FORBIDDEN, "ideen.nur_besitz"); }
    let text = clean(body.get("prompt").and_then(|t| t.as_str()).unwrap_or(""), MAX_PROMPT).replace('\n', " ");
    match state.store.set_idea_prompt(&v.pad, &text) {
        Ok(()) => { state.live.notify(&v.pad); Json(json!({ "ok": true, "prompt": text })).into_response() }
        Err(e) => { tracing::warn!("Frage setzen: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Live-Meldungen: nur „geändert“, ohne Inhalt; der Stand kommt über GET mit den Rechten der Person.
pub async fn live(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match viewer(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let rx = state.live.subscribe(&v.pad);
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
pub fn export(state: &AppState, pad: &str, format: &str, sp: Sprache) -> Option<Response> {
    let t = |k| texte::t(sp, k);
    let mut items = state.store.ideas(pad).ok()?;
    items.retain(|i| !i.hidden);
    items.sort_by(|a, b| b.votes.cmp(&a.votes).then(a.id.cmp(&b.id)));
    let prompt = state.store.idea_prompt(pad).unwrap_or_default();
    match format {
        "md" | "txt" => {
            let mut out = format!("# {}\n\n", if prompt.is_empty() { t("ideen.x.titel") } else { &prompt });
            for i in &items {
                let mark = if i.done { t("ideen.x.erledigt") } else { "" };
                out.push_str(&format!("- {} · {} {}{mark}\n", i.text.replace('\n', " "), i.votes, if i.votes == 1 { t("ideen.x.stimme") } else { t("ideen.x.stimmen") }));
            }
            let ct = if format == "md" { "text/markdown; charset=utf-8" } else { "text/plain; charset=utf-8" };
            Some(([(header::CONTENT_TYPE, ct)], out).into_response())
        }
        "csv" => {
            // Trennzeichen wie Tabellenprogramme in der Sprache: deutsch Semikolon, englisch Komma
            let sep = if sp == Sprache::En { ',' } else { ';' };
            let mut out = format!("\u{feff}{}\r\n", t("ideen.x.kopf"));
            for i in &items {
                let text = i.text.replace('"', "\"\"").replace('\n', " ");
                // Formeln in Tabellenprogrammen nicht ausführen lassen
                let text = if text.starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text };
                out.push_str(&format!("{}{sep}\"{text}\"{sep}{}{sep}{}\r\n", i.id, i.votes, if i.done { t("ideen.x.ja") } else { t("ideen.x.nein") }));
            }
            Some(([(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", t("ideen.x.datei")).as_str())], out).into_response())
        }
        _ => None,
    }
}

/// Seite der Sammlung; der Inhalt kommt per Skript, ohne Skript steht ein Hinweis da.
pub fn page(state: &AppState, sp: Sprache, url_id: &str, cfg: serde_json::Value) -> Response {
    let base = &state.config.base_path;
    let t = |k| texte::t(sp, k);
    let content = format!(r#"<main id="inhalt" class="ideas" tabindex="-1"><div class="wrap">
<section class="panel ideas-head" aria-labelledby="ideas-h">
<p class="kicker" id="view-kicker">{kicker}</p>
<div class="title-row"><h1 id="ideas-h"><span id="prompt">{titel}</span></h1><button type="button" class="icon-btn" id="prompt-edit" aria-controls="prompt-form" aria-expanded="false" hidden><svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3"/></svg><span class="visually-hidden">{frage_aendern}</span></button></div>
<form id="prompt-form" class="prompt-form" hidden><label class="field-label" for="prompt-input">{frage_label}</label><div class="field-row"><input id="prompt-input" maxlength="{MAX_PROMPT}" autocomplete="off"><button class="btn btn-secondary" type="submit">{speichern}</button></div></form>
<form id="add-form" class="add-form" hidden>
<label class="field-label" for="add-text">{idee_label}</label>
<textarea id="add-text" maxlength="{MAX_TEXT}" rows="3" required aria-describedby="add-hint add-error"></textarea>
<p class="field-error" id="add-error" role="alert" hidden></p>
<div class="add-row"><p class="hint" id="add-hint">{zeichen}</p><button class="btn btn-primary" type="submit">{abschicken}</button></div>
</form>
<p class="hint" id="readonly-hint" hidden>{nur_mitlesen}</p>
<noscript><p class="notice">{braucht_js}</p></noscript>
</section>
<div class="ideas-bar"><div class="sort" role="group" aria-label="{sortieren}"><button type="button" class="tab" data-sort="top" aria-pressed="true">{beliebt}</button><button type="button" class="tab" data-sort="new" aria-pressed="false">{neu}</button></div><p class="hint" id="ideas-count" aria-live="polite"></p></div>
<ol class="idea-list" id="open-list" aria-label="{offene}"></ol>
<p class="empty hint" id="empty" hidden>{leer_teilen}</p>
<details class="idea-group" id="done-box" hidden><summary>{erledigt} (<span id="done-n">0</span>)</summary><ol class="idea-list" id="done-list"></ol></details>
<details class="idea-group" id="hidden-box" hidden><summary>{ausgeblendet} (<span id="hidden-n">0</span>)</summary><p class="hint">{nur_fuer_dich}</p><ol class="idea-list" id="hidden-list"></ol></details>
<section class="panel ideas-share" aria-labelledby="share-h">
<h2 id="share-h">{teilen}</h2>
<p class="wall-row share-row"><button class="btn btn-secondary" type="button" id="share-copy">{link_kopieren}</button><span class="hint">{link_hinweis}</span></p>
<p class="wall-row"><a class="btn btn-secondary" href="{base}/p/{u}/leinwand" target="_blank" rel="noopener">{leinwand_oeffnen}</a><span class="hint">{leinwand_hinweis}</span></p>
<p class="hint">{sichern} <a href="{base}/p/{u}/export/md">Markdown</a> · <a href="{base}/p/{u}/export/csv">{csv}</a></p>
<p class="owner-tools" id="owner-tools" hidden><button type="button" class="btn-link" id="delete-all">{loeschen}</button></p>
</section>
</div></main>
<script type="application/json" id="nextpad-config">{cfg}</script>"#,
        u = web::urlencoding(url_id), cfg = cfg.to_string().replace("</", "<\\/"),
        kicker = t("ideen.s.kicker"), titel = t("ideen.s.titel"), frage_aendern = t("ideen.s.frage_aendern"), frage_label = t("ideen.s.frage_label"),
        speichern = t("ideen.s.speichern"), idee_label = t("ideen.s.idee_label"), abschicken = t("ideen.s.abschicken"), nur_mitlesen = t("ideen.nur_mitlesen"),
        zeichen = texte::tf(sp, "ideen.s.zeichen_html", &[("zaehler", r#"<span id="add-count">0</span>"#), ("max", &MAX_TEXT.to_string())]),
        braucht_js = t("ideen.s.braucht_js"), sortieren = t("ideen.s.sortieren"), beliebt = t("ideen.s.beliebt"), neu = t("ideen.s.neu"), offene = t("ideen.s.offene"),
        leer_teilen = t("ideen.s.leer_teilen"), erledigt = t("ideen.s.erledigt"), ausgeblendet = t("ideen.s.ausgeblendet"), nur_fuer_dich = t("ideen.s.nur_fuer_dich"),
        teilen = t("ideen.s.teilen"), link_kopieren = t("ideen.s.link_kopieren"), link_hinweis = t("ideen.s.link_hinweis"), leinwand_oeffnen = t("ideen.s.leinwand_oeffnen"),
        leinwand_hinweis = t("ideen.s.leinwand_hinweis"), sichern = t("ideen.s.sichern"), csv = t("ideen.s.csv"), loeschen = t("ideen.s.loeschen"));
    let head = format!(r#"<link rel="stylesheet" href="{base}/static/ideen.css?v={v}">"#, v = version());
    let scripts = format!(r#"<script src="{base}/static/ideen.js?v={v}"></script>"#, v = version());
    let html = web::page_frame(state, sp, Some(&format!("/p/{}", web::urlencoding(url_id))), t("ideen.s.seitentitel"), &head, &content, &scripts);
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
    let access = match web::check_access(&state, &headers, &id) { Ok(a) => a, Err((s, m)) => return web::abgelehnt(&headers, s, m) };
    if state.store.kind(&access.pad.id).as_deref() != Some(ADDON.art) { return web::abgelehnt(&headers, StatusCode::NOT_FOUND, "ideen.keine_sammlung"); }
    let base = &state.config.base_path;
    let sp = Sprache::aus_anfrage(&headers);
    let t = |k| texte::t(sp, k);
    // nach dem Link, nicht nach dem Cookie: wer die Leinwand direkt in einem frischen Browser öffnet, hat noch keins
    let join = if access.free && !id.starts_with("r.") {
        let origin = state.config.public_url.clone().unwrap_or_else(|| {
            let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
            format!("https://{host}{base}")
        });
        let url = format!("{}/p/{}", origin.trim_end_matches('/'), web::urlencoding(&access.pad.id));
        let qr = qrcode::QrCode::new(url.as_bytes()).map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(260, 260).quiet_zone(true).build()).unwrap_or_default();
        let qr = qr.split_once("<svg").map(|(_, r)| format!(r#"<svg role="img" aria-label="{}"{r}"#, t("ideen.s.qr"))).unwrap_or_default();
        format!(r#"<aside class="wall-join">{qr}<p>{}<br><strong>{}</strong></p></aside>"#, t("ideen.s.mitmachen"), html_escape::encode_text(&url))
    } else if !access.free {
        match state.store.join_url(&access.pad.id) {
            Some(u) => format!(r#"<aside class="wall-join">{}</aside>"#, crate::qr::block(&u, t("ideen.s.mitmachen_moodle"))),
            None => format!(r#"<aside class="wall-join"><p>{}</p></aside>"#, t("ideen.s.mitmachen_kurs")),
        }
    } else { String::new() };
    let brand = if state.config.theme == "hsd" && web::has_logo(&state) { format!(r#"<img class="wall-logo" src="{}" alt="{}">"#, web::theme_url(&state, "logo.png"), html_escape::encode_double_quoted_attribute(crate::look::get(&state).institution())) } else { format!(r#"<span class="wall-brand">{}{}</span>"#, web::logo_mark("wall-mark"), html_escape::encode_text(crate::look::get(&state).product())) };
    let cfg = json!({ "urlId": id, "basePath": base, "lang": sp.kuerzel(), "texte": texte::texte_mit(sp, "ideenjs.") });
    let icons = web::icon_links();
    let css = web::look_css(&state);
    let body = format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><meta name="color-scheme" content="light dark"><title>{leinwand_titel}</title>
<link rel="stylesheet" href="{base}/static/start.css?v={sv}"><link rel="stylesheet" href="{base}/static/ideen.css?v={v}">{css}</head>
<body class="wall"><main class="wall-main"><header class="wall-head"><div><p class="kicker">{name}</p><h1 id="prompt">{titel}</h1></div>{brand}</header>
<div class="wall-body"><ol class="wall-list" id="wall-list"></ol>{join}</div>
<p class="wall-empty" id="wall-empty" hidden>{leer}</p><p class="wall-count" id="wall-count"></p></main>
<script type="application/json" id="nextpad-config">{cfg}</script><script src="{base}/static/ideen-wand.js?v={v}"></script></body></html>"#,
        sv = web::start_version(), v = version(), cfg = cfg.to_string().replace("</", "<\\/"),
        lang = sp.kuerzel(), leinwand_titel = t("ideen.s.leinwand_titel"), name = t("ideen.name"), titel = t("ideen.s.titel"), leer = t("ideen.s.leer"));
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
