//! Anmeldung über Moodle per LTI 1.3: Moodle ist die Plattform, NextPad das Werkzeug.
//!
//! Ablauf (OIDC-Anmeldung, von Moodle angestoßen):
//! 1. Moodle schickt den Browser an `/lti/login` (iss, login_hint, target_link_uri …).
//! 2. NextPad merkt sich state und nonce, bindet state per Cookie an den Browser
//!    und leitet an den Autorisierungsendpunkt von Moodle weiter.
//! 3. Moodle schickt ein signiertes id_token per Formular an `/lti/launch`.
//! 4. NextPad prüft Signatur (Schlüssel der Plattform), Aussteller, Empfänger, Ablauf,
//!    nonce, Deployment und Nachrichtentyp, legt die Person als Autor an und setzt
//!    ein Anmelde-Cookie. Danach erscheinen Name und Profilbild aus Moodle in freien Pads.
//!
//! Als Kursaktivität (Parameter `typ=pad`, `typ=ideen` oder `typ=quiz` in der Werkzeugkonfiguration,
//! oder Zieladresse `…/lti/pad` bzw. `…/lti/ideen`): je Ressourcen-Link eine eigene Gruppe mit einem
//! Text-Pad, einer Ideensammlung oder einem Quiz-Raster. Zugang nur über den Start aus Moodle (Sitzung wie
//! bei Etherpad-Gruppenpads), Lehrende moderieren bzw. leiten.
use crate::texte::{t, tf, Sprache};
use crate::sperre::Sperre;
use crate::{ids, AppState};
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};
use jsonwebtoken::{jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex}};

/// Eine Moodle-Instanz, bei der NextPad als LTI-1.3-Werkzeug eingetragen ist.
#[derive(Clone, Debug, Deserialize)]
pub struct Platform {
    /// Kurzname für Protokolle
    pub name: String,
    /// Beschriftung der Schaltfläche auf der Startseite
    #[serde(default = "default_label")]
    pub label: String,
    /// Aussteller (`iss`), bei Moodle die wwwroot
    pub issuer: String,
    /// Client-ID aus der Werkzeugkonfiguration in Moodle
    pub client_id: String,
    /// Zulässige Deployment-IDs; leer: alle
    #[serde(default)]
    pub deployment_ids: Vec<String>,
    /// Autorisierungsendpunkt, bei Moodle `<wwwroot>/mod/lti/auth.php`
    pub auth_url: String,
    /// Öffentliche Schlüssel der Plattform, bei Moodle `<wwwroot>/mod/lti/certs.php`
    #[serde(default)]
    pub jwks_url: Option<String>,
    /// Alternativ die Schlüssel als Datei (wenn die Adresse vom Server aus nicht erreichbar ist)
    #[serde(default)]
    pub jwks_file: Option<PathBuf>,
    /// Einstieg für die Schaltfläche: Moodle-Adresse, die das Werkzeug startet
    #[serde(default)]
    pub start_url: Option<String>,
    /// Moodle-Adresse für Autorenzuordnung und Profilbilder; Standard: issuer
    #[serde(default)]
    pub moodle_url: Option<String>,
}

fn default_label() -> String { "Mit Moodle anmelden".into() }

impl Platform {
    pub fn moodle(&self) -> String { self.moodle_url.clone().unwrap_or_else(|| self.issuer.clone()).trim_end_matches('/').to_string() }
}

/// Plattformen aus einer JSON-Datei (Liste von Einträgen). Fehlt die Datei: keine LTI-Anmeldung.
pub fn load(path: &std::path::Path) -> anyhow::Result<Vec<Platform>> {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => anyhow::bail!("LTI-Datei {} nicht lesbar: {e}", path.display()),
    };
    let list: Vec<Platform> = serde_json::from_str(&s).map_err(|e| anyhow::anyhow!("LTI-Datei {}: {e}", path.display()))?;
    for p in &list {
        anyhow::ensure!(p.auth_url.starts_with("https://"), "LTI {}: auth_url muss https sein", p.name);
        anyhow::ensure!(p.jwks_url.is_some() || p.jwks_file.is_some(), "LTI {}: jwks_url oder jwks_file fehlt", p.name);
        if let Some(u) = &p.jwks_url { anyhow::ensure!(u.starts_with("https://"), "LTI {}: jwks_url muss https sein", p.name); }
        anyhow::ensure!(!p.client_id.is_empty() && !p.issuer.is_empty(), "LTI {}: issuer und client_id nötig", p.name);
    }
    Ok(list)
}

struct Pending { nonce: String, platform: usize, created: i64 }

/// Laufzeitzustand: offene Anmeldungen und zwischengespeicherte Plattformschlüssel.
#[derive(Default)]
pub struct LtiState {
    pending: Mutex<HashMap<String, Pending>>,
    jwks: Mutex<HashMap<usize, (i64, JwkSet)>>,
}

const PENDING_MS: i64 = 600_000;
const STATE_COOKIE: &str = "nextpad_lti";
/// Gültigkeit der Anmeldung in NextPad (Sekunden)
pub const LOGIN_SECONDS: i64 = 14 * 24 * 3600;

impl LtiState {
    pub fn purge(&self) {
        let now = ids::now_ms();
        self.pending.sperren().retain(|_, p| now - p.created < PENDING_MS);
    }
}

fn token32() -> String { format!("{}{}", ids::random16(), ids::random16()) }

fn enc(s: &str) -> String { serde_urlencoded::to_string([("x", s)]).map(|v| v[2..].to_string()).unwrap_or_default() }

/// Fehlerseite der Anmeldung; `msg` ist schon in der Sprache `sp`.
fn page(sp: Sprache, status: StatusCode, msg: &str) -> Response {
    (status, Html(format!("<!doctype html><html lang=\"{}\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>NextPad</title><body style=\"font-family:Arial,sans-serif;padding:2em;max-width:36em;line-height:1.5\"><p>{}</p><p><a href=\"./..\">{}</a></p></body></html>", sp.kuerzel(), html_escape::encode_text(msg), t(sp, "fehler.zur_startseite")))).into_response()
}

fn base_path(state: &AppState) -> String { if state.config.base_path.is_empty() { "/".into() } else { format!("{}/", state.config.base_path) } }

/// Schritt 1: Anmeldung aus Moodle anstoßen (GET oder POST).
pub async fn login(State(state): State<Arc<AppState>>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, body: Bytes) -> Response {
    let sp = Sprache::aus_anfrage(&headers);
    let Some(public) = state.config.public_url.clone() else { return page(sp, StatusCode::NOT_FOUND, t(sp, "lti.nicht_eingerichtet")) };
    let mut p = q;
    if !body.is_empty() {
        if let Ok(form) = serde_urlencoded::from_bytes::<Vec<(String, String)>>(&body) { p.extend(form); }
    }
    let get = |k: &str| p.get(k).map(String::as_str).filter(|v| !v.is_empty());
    let (Some(iss), Some(login_hint), Some(target)) = (get("iss"), get("login_hint"), get("target_link_uri")) else {
        return page(sp, StatusCode::BAD_REQUEST, t(sp, "lti.unvollstaendig"));
    };
    let iss = iss.trim_end_matches('/');
    let found = state.config.lti.iter().position(|pl| pl.issuer.trim_end_matches('/') == iss && get("client_id").map(|c| c == pl.client_id).unwrap_or(true));
    let Some(idx) = found else {
        tracing::warn!("LTI-Anmeldung von unbekannter Plattform");
        return page(sp, StatusCode::FORBIDDEN, t(sp, "lti.plattform"));
    };
    let pl = &state.config.lti[idx];
    if let Some(dep) = get("lti_deployment_id") {
        if !pl.deployment_ids.is_empty() && !pl.deployment_ids.iter().any(|d| d == dep) {
            return page(sp, StatusCode::FORBIDDEN, t(sp, "lti.deployment"));
        }
    }
    if !target.starts_with(&public) {
        return page(sp, StatusCode::BAD_REQUEST, t(sp, "lti.ziel"));
    }
    let st = token32();
    let nonce = token32();
    {
        let mut m = state.lti.pending.sperren();
        let now = ids::now_ms();
        m.retain(|_, p| now - p.created < PENDING_MS);
        if m.len() > 10_000 { return page(sp, StatusCode::SERVICE_UNAVAILABLE, t(sp, "lti.zu_viele")); }
        m.insert(st.clone(), Pending { nonce: nonce.clone(), platform: idx, created: now });
    }
    let redirect_uri = format!("{public}/lti/launch");
    let mut url = format!(
        "{}{}scope=openid&response_type=id_token&response_mode=form_post&prompt=none&client_id={}&redirect_uri={}&login_hint={}&state={}&nonce={}",
        pl.auth_url, if pl.auth_url.contains('?') { "&" } else { "?" }, enc(&pl.client_id), enc(&redirect_uri), enc(login_hint), st, nonce
    );
    if let Some(h) = get("lti_message_hint") { url.push_str(&format!("&lti_message_hint={}", enc(h))); }
    // Cookie bindet die Anmeldung an diesen Browser (Schutz gegen untergeschobene Anmeldungen).
    // SameSite=None, weil der Rücksprung ein seitenübergreifendes POST von Moodle ist.
    let cookie = format!("{STATE_COOKIE}={st}; Path={}lti/; Max-Age=600; HttpOnly; Secure; SameSite=None", base_path(&state));
    let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, url)]).into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) { resp.headers_mut().append(header::SET_COOKIE, v); }
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

#[derive(Deserialize)]
pub struct LaunchForm {
    id_token: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    nonce: Option<String>,
    azp: Option<String>,
    aud: serde_json::Value,
    name: Option<String>,
    given_name: Option<String>,
    email: Option<String>,
    family_name: Option<String>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/deployment_id")]
    deployment_id: Option<String>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/message_type")]
    message_type: Option<String>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/version")]
    version: Option<String>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/resource_link", default)]
    resource_link: Option<LinkClaim>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/context", default)]
    context: Option<LinkClaim>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/roles", default)]
    roles: Vec<String>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/custom", default)]
    custom: Option<HashMap<String, serde_json::Value>>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/target_link_uri", default)]
    target_link_uri: Option<String>,
    #[serde(rename = "https://purl.imsglobal.org/spec/lti/claim/launch_presentation", default)]
    launch_presentation: Option<Praesentation>,
}

#[derive(Deserialize)]
struct Praesentation {
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Deserialize)]
struct LinkClaim {
    id: String,
    #[serde(default)]
    title: Option<String>,
}

impl Claims {
    /// Typ der Kursaktivität: "pad" oder die Kennung eines Addons; None: nur anmelden.
    fn activity(&self) -> Option<&'static str> {
        let typ = self.custom.as_ref().and_then(|c| c.get("typ")).and_then(|v| v.as_str()).map(str::trim);
        let target = self.target_link_uri.as_deref().map(|u| u.split(['?', '#']).next().unwrap_or("").trim_end_matches('/').to_string()).unwrap_or_default();
        match typ {
            Some("pad" | "text") => Some("pad"),
            Some(t) if crate::addons::nach_kennung(t).is_some_and(|a| a.moodle.is_some()) => crate::addons::nach_kennung(t).map(|a| a.kennung),
            _ if target.ends_with("/lti/ideen") => Some("ideen"),
            _ if target.ends_with("/lti/pad") => Some("pad"),
            _ => None,
        }
    }
    /// Admins von NextPad: Administration der Moodle-Instanz oder in NEXTPAD_ADMINS (Mailadressen).
    fn administers(&self, admins: &[String]) -> bool {
        self.roles.iter().any(|r| r.ends_with("/system/person#Administrator") || r.ends_with("/institution/person#Administrator"))
            || self.email.as_deref().map(|e| e.trim().to_ascii_lowercase()).is_some_and(|e| admins.contains(&e))
    }
    /// Lehrende, Assistenz und Verwaltung moderieren.
    fn moderates(&self) -> bool {
        self.roles.iter().any(|r| r.ends_with("#Instructor") || r.ends_with("#TeachingAssistant") || r.ends_with("#ContentDeveloper") || r.ends_with("#Administrator"))
    }
}

fn fetch_jwks(pl: &Platform) -> Result<JwkSet, String> {
    if let Some(f) = &pl.jwks_file {
        let s = std::fs::read_to_string(f).map_err(|e| format!("Schlüsseldatei: {e}"))?;
        return serde_json::from_str(&s).map_err(|e| format!("Schlüsseldatei: {e}"));
    }
    let url = pl.jwks_url.as_deref().ok_or("keine Schlüsseladresse")?;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(10))).build().into();
    let mut r = agent.get(url).call().map_err(|e| format!("Schlüsselabruf: {e}"))?;
    let s = r.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| format!("Schlüsselabruf: {e}"))?;
    serde_json::from_str(&s).map_err(|e| format!("Schlüssel: {e}"))
}

/// Schlüssel zur Kennung `kid`. Zwischenspeicher eine Stunde; bei unbekannter Kennung
/// neu laden (Schlüsselwechsel in Moodle), höchstens einmal je Minute.
fn key_for(state: &AppState, idx: usize, kid: Option<&str>) -> Result<DecodingKey, String> {
    let pl = &state.config.lti[idx];
    let now = ids::now_ms();
    let pick = |set: &JwkSet| match kid { Some(k) => set.find(k).cloned(), None if set.keys.len() == 1 => set.keys.first().cloned(), None => None };
    let cached = state.lti.jwks.sperren().get(&idx).cloned().filter(|(t, _)| now - *t < 3_600_000);
    if let Some((t, set)) = &cached {
        if let Some(j) = pick(set) { return DecodingKey::from_jwk(&j).map_err(|e| format!("Schlüssel: {e}")); }
        if now - *t < 60_000 { return Err("Signaturschlüssel der Plattform unbekannt".into()); }
    }
    let set = fetch_jwks(pl)?;
    state.lti.jwks.sperren().insert(idx, (now, set.clone()));
    let j = pick(&set).ok_or("Signaturschlüssel der Plattform unbekannt")?;
    DecodingKey::from_jwk(&j).map_err(|e| format!("Schlüssel: {e}"))
}

fn verify(state: &AppState, idx: usize, token: &str, nonce: &str) -> Result<Claims, String> {
    let pl = &state.config.lti[idx];
    let head = jsonwebtoken::decode_header(token).map_err(|e| format!("Token: {e}"))?;
    if head.alg != Algorithm::RS256 { return Err("Token: Verfahren nicht erlaubt".into()); }
    let key = key_for(state, idx, head.kid.as_deref())?;
    let mut v = Validation::new(Algorithm::RS256);
    v.set_audience(&[&pl.client_id]);
    v.set_issuer(&[pl.issuer.as_str(), pl.issuer.trim_end_matches('/')]);
    v.leeway = 60;
    v.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
    let c = jsonwebtoken::decode::<Claims>(token, &key, &v).map_err(|e| format!("Token: {e}"))?.claims;
    if c.nonce.as_deref() != Some(nonce) { return Err("Token: nonce stimmt nicht".into()); }
    let multi_aud = c.aud.as_array().map(|a| a.len() > 1).unwrap_or(false);
    if (multi_aud || c.azp.is_some()) && c.azp.as_deref() != Some(pl.client_id.as_str()) { return Err("Token: azp stimmt nicht".into()); }
    let dep = c.deployment_id.as_deref().ok_or("Token: Deployment fehlt")?;
    if !pl.deployment_ids.is_empty() && !pl.deployment_ids.iter().any(|d| d == dep) { return Err("Token: Deployment nicht freigeschaltet".into()); }
    if c.version.as_deref() != Some("1.3.0") { return Err("Token: LTI-Version nicht 1.3.0".into()); }
    if c.message_type.as_deref() != Some("LtiResourceLinkRequest") { return Err("Token: Nachrichtentyp nicht unterstützt".into()); }
    if c.sub.is_empty() || c.sub.len() > 255 { return Err("Token: sub ungültig".into()); }
    Ok(c)
}

/// Schritt 3: Rücksprung von Moodle mit id_token.
pub async fn launch(State(state): State<Arc<AppState>>, headers: HeaderMap, axum::Form(f): axum::Form<LaunchForm>) -> Response {
    let sp = Sprache::aus_anfrage(&headers);
    if let Some(e) = f.error {
        tracing::info!("LTI-Anmeldung von Moodle abgelehnt: {e} {}", f.error_description.as_deref().unwrap_or(""));
        let msg = if e == "login_required" { t(sp, "lti.nicht_angemeldet") } else { t(sp, "lti.abgelehnt") };
        return page(sp, StatusCode::FORBIDDEN, msg);
    }
    let (Some(token), Some(st)) = (f.id_token, f.state) else { return page(sp, StatusCode::BAD_REQUEST, t(sp, "lti.antwort")) };
    let pending = state.lti.pending.sperren().remove(&st);
    let Some(pending) = pending.filter(|p| ids::now_ms() - p.created < PENDING_MS) else {
        return page(sp, StatusCode::BAD_REQUEST, t(sp, "lti.abgelaufen"));
    };
    let bound = crate::web::cookie_values(&headers, STATE_COOKIE).iter().any(|c| c.as_bytes() == st.as_bytes());
    if !bound {
        return page(sp, StatusCode::BAD_REQUEST, t(sp, "lti.browser"));
    }
    let idx = pending.platform;
    let st2 = state.clone();
    let res = tokio::task::spawn_blocking(move || verify(&st2, idx, &token, &pending.nonce)).await.unwrap_or_else(|_| Err("Prüfung abgebrochen".into()));
    let claims = match res {
        Ok(c) => c,
        Err(e) => { tracing::warn!("LTI-Anmeldung über {} abgewiesen: {e}", state.config.lti[idx].name); return page(sp, StatusCode::FORBIDDEN, t(sp, "lti.unbestaetigt")); }
    };
    // ab hier gilt: eigene Wahl, sonst die Sprache aus Moodle, sonst die der Anfrage
    let sp = Sprache::gewaehlt(&headers).or_else(|| claims.launch_presentation.as_ref().and_then(|p| p.locale.as_deref()).and_then(Sprache::aus_kuerzel)).unwrap_or(sp);
    let pl = &state.config.lti[idx];
    let name = claims.name.clone().filter(|n| !n.trim().is_empty())
        .or_else(|| { let n = format!("{} {}", claims.given_name.as_deref().unwrap_or(""), claims.family_name.as_deref().unwrap_or("")); (!n.trim().is_empty()).then(|| n.trim().to_string()) })
        .unwrap_or_else(|| "Moodle-Konto".into());
    let name: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
    let author = match state.store.create_author_from(&name, Some(&claims.sub), Some(&pl.moodle())) {
        Ok(a) => a,
        Err(e) => { tracing::error!("LTI-Autor: {e}"); return page(sp, StatusCode::INTERNAL_SERVER_ERROR, t(sp, "lti.fehlgeschlagen")); }
    };
    // Mailadresse nur für die Ankündigung vor dem Löschen eigener Pads
    let email = claims.email.as_deref().map(str::trim).filter(|e| e.contains('@') && e.len() <= 254 && !e.chars().any(|c| c.is_control() || c.is_whitespace()));
    let _ = state.store.set_author_email(&author.id, email);
    let token = match state.store.create_login(&author.id, ids::now_ms() / 1000 + LOGIN_SECONDS, claims.administers(&state.config.admins)) {
        Ok(t) => t,
        Err(e) => { tracing::error!("LTI-Anmeldung speichern: {e}"); return page(sp, StatusCode::INTERNAL_SERVER_ERROR, t(sp, "lti.fehlgeschlagen")); }
    };
    tracing::info!("LTI-Anmeldung über {}{}", pl.name, if claims.administers(&state.config.admins) { " (Admin)" } else { "" });
    let path = base_path(&state);
    let mut target = path.clone();
    let mut session_cookie = None;
    if let Some(typ) = claims.activity() {
        let addon = crate::addons::nach_kennung(typ);
        if let Some(a) = addon.filter(|a| !crate::addons::aktiv(&state, a)) {
            return page(sp, StatusCode::NOT_FOUND, &tf(sp, "lti.addon_aus", &[("name", t(sp, a.name))]));
        }
        let kind = addon.map_or("pad", |a| a.art);
        let (Some(ctx), Some(link)) = (&claims.context, &claims.resource_link) else {
            return page(sp, StatusCode::BAD_REQUEST, t(sp, "lti.kontext"));
        };
        let dep = claims.deployment_id.as_deref().unwrap_or("");
        let key = serde_json::json!([pl.issuer.trim_end_matches('/'), dep, ctx.id, link.id]).to_string();
        let pad = match state.store.pad_for_link(&key) {
            Ok(Some(p)) => p,
            Ok(None) => match state.store.create_link_pad(&key, kind) {
                Ok(p) => {
                    let title: String = link.title.as_deref().unwrap_or("").chars().filter(|c| !c.is_control()).take(200).collect();
                    if !title.trim().is_empty() {
                        if let Some(setzen) = addon.and_then(|a| a.moodle) { let _ = setzen(&state.store, &p, title.trim()); }
                    }
                    tracing::info!("Kursaktivität über {} angelegt", pl.name);
                    p
                }
                Err(e) => { tracing::error!("Kursaktivität anlegen: {e}"); return page(sp, StatusCode::INTERNAL_SERVER_ERROR, t(sp, "lti.aktivitaet")); }
            },
            Err(e) => { tracing::error!("Kursaktivität suchen: {e}"); return page(sp, StatusCode::INTERNAL_SERVER_ERROR, t(sp, "lti.fehlgeschlagen")); }
        };
        if claims.moderates() { let _ = state.store.add_moderator(&pad, &author.id); }
        // QR-Code am Beamer: Moodle zählt Ressourcen-Links mit der Nummer der LTI-Instanz, die Aktivität ist dann
        // <Moodle>/mod/lti/view.php?l=<Nummer>; wer scannt, meldet sich in Moodle an und startet die Aktivität
        if !link.id.is_empty() && link.id.len() <= 18 && link.id.chars().all(|c| c.is_ascii_digit()) {
            let _ = state.store.set_join_url(&pad, &format!("{}/mod/lti/view.php?l={}", pl.moodle(), link.id));
        }
        // „Zurück zum Kurs“: Moodle schickt als Kontext die Kursnummer
        if !ctx.id.is_empty() && ctx.id.len() <= 18 && ctx.id.chars().all(|c| c.is_ascii_digit()) {
            let _ = state.store.set_course_url(&pad, &format!("{}/course/view.php?id={}", pl.moodle(), ctx.id));
        }
        let group = crate::ids::group_of_pad(&pad).unwrap_or("").to_string();
        let sid = match state.store.create_session(&group, &author.id, ids::now_ms() / 1000 + LOGIN_SECONDS) {
            Ok(s) => s,
            Err(e) => { tracing::error!("Sitzung anlegen: {e}"); return page(sp, StatusCode::INTERNAL_SERVER_ERROR, t(sp, "lti.fehlgeschlagen")); }
        };
        // mehrere Sitzungen in einem Cookie, durch Komma getrennt wie bei Etherpad; ältere bleiben gültig
        let now_s = ids::now_ms() / 1000;
        let mut sids: Vec<String> = crate::web::cookie_values(&headers, &state.config.cookie_name).into_iter()
            .filter(|v| v.starts_with("s.") && v.len() <= 20 && state.store.get_session(v).ok().flatten().is_some_and(|s| s.valid_until > now_s && s.group_id != group))
            .collect();
        sids.push(sid);
        if sids.len() > 30 { sids.drain(..sids.len() - 30); }
        // SameSite=None, damit die Aktivität auch eingebettet in Moodle funktioniert
        session_cookie = Some(format!("{}={}; Path={path}; Max-Age={LOGIN_SECONDS}; HttpOnly; Secure; SameSite=None", state.config.cookie_name, sids.join(",")));
        target = format!("{path}p/{}", crate::web::urlencoding(&pad));
    }
    let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, target)]).into_response();
    let h = resp.headers_mut();
    if let Some(c) = session_cookie { if let Ok(v) = HeaderValue::from_str(&c) { h.append(header::SET_COOKIE, v); } }
    for c in [
        format!("{}={token}; Path={path}; Max-Age={LOGIN_SECONDS}; HttpOnly; Secure; SameSite=Lax", state.config.login_cookie),
        format!("{STATE_COOKIE}=; Path={path}lti/; Max-Age=0; HttpOnly; Secure; SameSite=None"),
    ] { if let Ok(v) = HeaderValue::from_str(&c) { h.append(header::SET_COOKIE, v); } }
    // Sprache aus Moodle bei jedem Start auffrischen; eine Sprache, die NextPad nicht kann, löscht sie
    if let Some(locale) = claims.launch_presentation.as_ref().and_then(|p| p.locale.as_deref()) {
        let c = match crate::texte::Sprache::aus_kuerzel(locale) {
            Some(sp) => format!("{}={}; Path={path}; Max-Age=31536000; HttpOnly; Secure; SameSite=None", crate::texte::MOODLE_COOKIE, sp.kuerzel()),
            None => format!("{}=; Path={path}; Max-Age=0; HttpOnly; Secure; SameSite=None", crate::texte::MOODLE_COOKIE),
        };
        if let Ok(v) = HeaderValue::from_str(&c) { h.append(header::SET_COOKIE, v); }
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Öffentliche Schlüssel des Werkzeugs. NextPad nutzt keine LTI-Dienste und signiert nichts,
/// Moodle verlangt aber eine Schlüsseladresse in der Werkzeugkonfiguration.
pub async fn jwks() -> Response {
    ([(header::CONTENT_TYPE, "application/json")], r#"{"keys":[]}"#).into_response()
}

/// Abmelden: Anmeldung löschen, Cookie entfernen. Nur per POST von der eigenen Seite.
pub async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !crate::web::origin_ok(&state, &headers) { return StatusCode::FORBIDDEN.into_response(); }
    for t in crate::web::cookie_values(&headers, &state.config.login_cookie) { let _ = state.store.delete_login(&t); }
    let path = base_path(&state);
    let mut resp = (StatusCode::SEE_OTHER, [(header::LOCATION, path.clone())]).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("{}=; Path={path}; Max-Age=0; HttpOnly; Secure; SameSite=Lax", state.config.login_cookie)) { resp.headers_mut().append(header::SET_COOKIE, v); }
    resp
}
