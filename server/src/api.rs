//! Etherpad-kompatible HTTP-API (`/api`, `/api/1.2*/<Funktion>`), Antwort `{code, message, data}`.
use crate::{ids, store::CreatePadError, AppState};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{header, HeaderMap, Method},
    response::{IntoResponse, Json, Response},
};
use serde_json::{json, Value};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use subtle::ConstantTimeEq;

type Params = HashMap<String, String>;

fn reply(code: i32, message: &str, data: Value) -> Response {
    Json(json!({ "code": code, "message": message, "data": data })).into_response()
}
fn ok(data: Value) -> Response { reply(0, "ok", data) }
fn bad(message: &str) -> Response { reply(1, message, Value::Null) }
fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("API-Fehler: {e}");
    reply(2, "internal error", Value::Null)
}

pub async fn version(State(state): State<Arc<AppState>>) -> Response {
    Json(json!({ "currentVersion": state.config.api_version })).into_response()
}

/// Index des passenden Schlüssels (Vergleich in konstanter Zeit über alle Schlüssel).
fn key_index(state: &AppState, key: &str) -> Option<usize> {
    let k = key.as_bytes();
    let mut found = None;
    for (i, known) in state.config.api_keys.iter().enumerate() {
        if known.len() == k.len() && bool::from(known.ct_eq(k)) && found.is_none() { found = Some(i); }
    }
    found
}

async fn collect_params(headers: &HeaderMap, query: Params, body: Bytes) -> Params {
    let mut p = query;
    let ct_raw = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let ct = ct_raw.to_ascii_lowercase();
    if ct.starts_with("application/x-www-form-urlencoded") {
        if let Ok(form) = serde_urlencoded::from_bytes::<Vec<(String, String)>>(&body) {
            for (k, v) in form { p.insert(k, v); }
        }
    } else if ct.starts_with("multipart/form-data") {
        // multipart ohne zusätzliche Bibliothek: einfacher Parser für Textfelder
        // Boundary aus dem Originalwert (Groß-/Kleinschreibung zählt)
        if let Some(boundary) = ct_raw.split("boundary=").nth(1).map(|b| b.split(';').next().unwrap_or("").trim().trim_matches('"').to_string()) {
            let delim = format!("--{boundary}");
            let text = String::from_utf8_lossy(&body);
            for part in text.split(&delim) {
                let part = part.trim_start_matches("\r\n");
                if part.is_empty() || part.starts_with("--") { continue; }
                let Some((head, value)) = part.split_once("\r\n\r\n") else { continue };
                let name = head.split("name=\"").nth(1).and_then(|s| s.split('"').next());
                if let Some(name) = name {
                    let value = value.strip_suffix("\r\n").unwrap_or(value);
                    p.insert(name.to_string(), value.to_string());
                }
            }
        }
    } else if ct.starts_with("application/json") {
        if let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(&body) {
            for (k, v) in m {
                p.insert(k, match v { Value::String(s) => s, other => other.to_string() });
            }
        }
    }
    p
}

pub async fn call(
    State(state): State<Arc<AppState>>,
    Path((version, func)): Path<(String, String)>,
    Query(query): Query<Params>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    _method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let ip = crate::net::client_ip(&state, &headers, peer);
    if !crate::net::in_list(&state.config.api_allowed, ip) {
        tracing::warn!(%ip, %func, "API-Aufruf von nicht erlaubter Adresse");
        return reply(4, "no or wrong API Key", Value::Null);
    }
    if !version.starts_with("1.") {
        return reply(3, "no such api version", Value::Null);
    }
    let mut p = collect_params(&headers, query, body).await;
    // Browser-FormData liefert CRLF; Pads arbeiten mit LF
    for k in ["text", "html"] { if let Some(v) = p.get_mut(k) { *v = v.replace("\r\n", "\n"); } }
    let key = p.get("apikey").cloned().unwrap_or_default();
    let Some(key_idx) = key_index(&state, &key) else {
        tracing::warn!(%ip, %func, "API-Aufruf mit falschem Schlüssel");
        return reply(4, "no or wrong API Key", Value::Null);
    };
    let moodle = state.config.api_key_moodle.get(key_idx).cloned().flatten();
    let get = |k: &str| p.get(k).cloned();
    let st = &state.store;
    match func.as_str() {
        "checkToken" => ok(Value::Null),
        "createGroup" => match st.create_group() {
            Ok(id) => ok(json!({ "groupID": id })),
            Err(e) => internal(e),
        },
        "deleteGroup" => {
            let Some(g) = get("groupID") else { return bad("groupID is required") };
            match st.group_exists(&g) {
                Ok(false) => bad("groupID does not exist"),
                Ok(true) => match st.delete_group(&g) {
                    Ok(pads) => { for pid in pads { state.rooms.evict(&pid); } ok(Value::Null) }
                    Err(e) => internal(e),
                },
                Err(e) => internal(e),
            }
        }
        "createGroupPad" => {
            let (Some(g), Some(name)) = (get("groupID"), get("padName")) else { return bad("groupID and padName are required") };
            if name.contains('$') || name.contains('/') || name.is_empty() { return bad("padName contains invalid characters"); }
            let pad_id = format!("{g}${name}");
            match st.create_pad(&pad_id, Some(&g)) {
                Ok(_) => {
                    if let Some(text) = get("text") {
                        if let Err(e) = state.rooms.set_text(&pad_id, &text).await { return internal(e); }
                    }
                    ok(json!({ "padID": pad_id }))
                }
                Err(CreatePadError::Exists) => bad("padName does already exist"),
                Err(CreatePadError::NoSuchGroup) => bad("groupID does not exist"),
                Err(CreatePadError::Other(e)) => internal(e),
            }
        }
        "createPad" => {
            if !state.config.allow_free_pads { return bad("free pads are disabled on this server"); }
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            if !crate::web::valid_free_pad_name(&pad_id) { return bad("padID contains invalid characters"); }
            match st.create_pad(&pad_id, None) {
                Ok(_) => {
                    if let Some(text) = get("text") { if let Err(e) = state.rooms.set_text(&pad_id, &text).await { return internal(e); } }
                    ok(Value::Null)
                }
                Err(CreatePadError::Exists) => bad("padID does already exist"),
                Err(e) => internal(format!("{e:?}")),
            }
        }
        "deletePad" => {
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            match st.delete_pad(&pad_id) {
                Ok(true) => { state.rooms.evict(&pad_id); ok(Value::Null) }
                Ok(false) => bad("padID does not exist"),
                Err(e) => internal(e),
            }
        }
        "createAuthor" => match st.create_author(get("name").as_deref().unwrap_or(""), None) {
            Ok(a) => ok(json!({ "authorID": a.id })),
            Err(e) => internal(e),
        },
        "createAuthorIfNotExistsFor" => {
            let Some(mapper) = get("authorMapper") else { return bad("authorMapper is required") };
            match st.create_author_from(get("name").as_deref().unwrap_or(""), Some(&mapper), moodle.as_deref()) {
                Ok(a) => ok(json!({ "authorID": a.id })),
                Err(e) => internal(e),
            }
        }
        "getAuthorName" => {
            let Some(id) = get("authorID") else { return bad("authorID is required") };
            match st.get_author(&id) { Ok(Some(a)) => ok(json!(a.name)), Ok(None) => bad("authorID does not exist"), Err(e) => internal(e) }
        }
        "createSession" => {
            let (Some(g), Some(a), Some(v)) = (get("groupID"), get("authorID"), get("validUntil")) else { return bad("groupID, authorID and validUntil are required") };
            let Ok(valid_until) = v.trim().parse::<i64>() else { return bad("validUntil is not a number") };
            let now_s = ids::now_ms() / 1000;
            if valid_until <= now_s { return bad("validUntil is in the past"); }
            match (st.group_exists(&g), st.get_author(&a)) {
                (Ok(false), _) => bad("groupID does not exist"),
                (_, Ok(None)) => bad("authorID does not exist"),
                (Ok(true), Ok(Some(_))) => match st.create_session(&g, &a, valid_until) {
                    Ok(id) => ok(json!({ "sessionID": id })),
                    Err(e) => internal(e),
                },
                (Err(e), _) | (_, Err(e)) => internal(e),
            }
        }
        "deleteSession" => {
            let Some(id) = get("sessionID") else { return bad("sessionID is required") };
            match st.delete_session(&id) { Ok(true) => ok(Value::Null), Ok(false) => bad("sessionID does not exist"), Err(e) => internal(e) }
        }
        "getSessionInfo" => {
            let Some(id) = get("sessionID") else { return bad("sessionID is required") };
            match st.get_session(&id) {
                Ok(Some(s)) => ok(json!({ "groupID": s.group_id, "authorID": s.author_id, "validUntil": s.valid_until })),
                Ok(None) => bad("sessionID does not exist"),
                Err(e) => internal(e),
            }
        }
        "getReadOnlyID" => {
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            match st.get_pad(&pad_id) { Ok(Some(p)) => ok(json!({ "readOnlyID": p.readonly_id })), Ok(None) => bad("padID does not exist"), Err(e) => internal(e) }
        }
        "getHTML" | "getText" => {
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            match st.get_pad(&pad_id) {
                Ok(None) => bad("padID does not exist"),
                Err(e) => internal(e),
                Ok(Some(_)) => {
                    if func == "getHTML" {
                        match state.rooms.html(&pad_id).await { Ok(h) => ok(json!({ "html": h })), Err(e) => internal(e) }
                    } else {
                        match state.rooms.text(&pad_id).await { Ok(t) => ok(json!({ "text": t })), Err(e) => internal(e) }
                    }
                }
            }
        }
        "setText" | "setHTML" => {
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            match st.get_pad(&pad_id) {
                Ok(None) => bad("padID does not exist"),
                Err(e) => internal(e),
                Ok(Some(_)) => {
                    let r = if func == "setText" {
                        let Some(t) = get("text") else { return bad("text is required") };
                        state.rooms.set_text(&pad_id, &t).await
                    } else {
                        let Some(h) = get("html") else { return bad("html is required") };
                        state.rooms.set_html(&pad_id, &h).await
                    };
                    match r { Ok(()) => ok(Value::Null), Err(e) => internal(e) }
                }
            }
        }
        "getRevisionsCount" => {
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            match st.get_pad(&pad_id) {
                Ok(None) => bad("padID does not exist"),
                Err(e) => internal(e),
                Ok(Some(_)) => match st.update_count(&pad_id) { Ok(n) => ok(json!({ "revisions": n })), Err(e) => internal(e) },
            }
        }
        "getLastEdited" => {
            let Some(pad_id) = get("padID") else { return bad("padID is required") };
            match st.get_pad(&pad_id) { Ok(Some(p)) => ok(json!({ "lastEdited": p.last_edited })), Ok(None) => bad("padID does not exist"), Err(e) => internal(e) }
        }
        "listAllPads" => match st.list_pads() { Ok(l) => ok(json!({ "padIDs": l })), Err(e) => internal(e) },
        _ => reply(3, "no such function", Value::Null),
    }
}
