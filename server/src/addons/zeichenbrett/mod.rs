//! Zeichenbrett: Excalidraw über den Yjs-Server des Kerns. Das Addon bringt nur Seite und Bündel mit;
//! Verbindung, Speichern und Zeitleiste laufen wie beim Text-Pad.
use super::{Addon, Seite};
use crate::{texte::{self, Sprache}, web, AppState};
use axum::{
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use std::sync::Arc;

const TEXTE_DE: &[(&str, &str)] = &[
    ("zeichenbrett.sichern", "Zeichenbretter lassen sich im Zeichenbrett über das Menü als PNG oder SVG sichern."),
    ("zeichenbrett.name", "Zeichenbrett"),
    ("zeichenbrett.kachel_html", r#"<li><a class="template" href="{base}/new?typ=zeichenbrett"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3M4 20h16"/></svg><span class="template-name">Zeichenbrett</span><span class="template-desc">Gemeinsam skizzieren</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>"#),
    ("zeichenbrett.hilfe_html", r#"<h2>Zeichenbrett</h2><p>„Zeichenbrett“ legt eine gemeinsame Zeichenfläche an: Formen, Pfeile, Freihand und Text, alle sehen die Zeiger der anderen. Gesichert wird über das Menü links oben als PNG, SVG oder Excalidraw-Datei. Bilder einfügen ist noch nicht möglich. Die Zeichenfläche ist mit Screenreadern kaum nutzbar; für zugängliche Zusammenarbeit eignet sich ein Text-Pad.</p>"#),
    ("zeichenbrett.export_formate", "Zeichenbretter lassen sich im Zeichenbrett über das Menü als PNG, SVG oder Excalidraw-Datei sichern."),
];
const TEXTE_EN: &[(&str, &str)] = &[
    ("zeichenbrett.sichern", "Whiteboards can be saved as PNG or SVG from the whiteboard’s menu."),
    ("zeichenbrett.name", "Whiteboard"),
    ("zeichenbrett.kachel_html", r#"<li><a class="template" href="{base}/new?typ=zeichenbrett"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M4 20l4-1L19 8l-3-3L5 16zM14 7l3 3M4 20h16"/></svg><span class="template-name">Whiteboard</span><span class="template-desc">Sketch together</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>"#),
    ("zeichenbrett.hilfe_html", r#"<h2>Whiteboard</h2><p>“Whiteboard” creates a shared drawing area: shapes, arrows, freehand and text; everyone sees the others’ pointers. It is saved from the menu at the top left as PNG, SVG or Excalidraw file. Inserting images is not possible yet. The whiteboard is hardly usable with screen readers; for accessible collaboration, a text pad is the better choice.</p>"#),
    ("zeichenbrett.export_formate", "Whiteboards can be saved from the whiteboard menu as PNG, SVG or Excalidraw file."),
];

pub const ADDON: Addon = Addon {
    kennung: "zeichenbrett",
    art: "board",
    name: "zeichenbrett.name",
    daten: None,
    routen,
    seite,
    export,
    kachel: "zeichenbrett.kachel_html",
    sichern: "zeichenbrett.sichern",
    hilfe: "zeichenbrett.hilfe_html",
    moodle: None,
    texte: (TEXTE_DE, TEXTE_EN),
};


fn routen() -> Router<Arc<AppState>> { Router::new().route("/static/board/{*path}", get(static_board)) }

/// Zeichenbrett-Bündel (Excalidraw, Teile, Schriften), beim Bau eingebettet.
static BOARD: include_dir::Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/static/board");

fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let mut h: u64 = 1469598103934665603;
        for f in ["board.js", "board.css"] { if let Some(file) = BOARD.get_file(f) { for b in file.contents() { h ^= *b as u64; h = h.wrapping_mul(1099511628211); } } }
        format!("{h:016x}")
    })
}

fn seite(s: Seite) -> Response {
    let state = s.state;
    let base = &state.config.base_path;
    let icons = web::icon_links();
    let body = format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8">{icons}<meta name="viewport" content="width=device-width, initial-scale=1"><title>NextPad · Zeichenbrett</title>
<link rel="stylesheet" href="{base}/static/board/board.css?v={v}"><style>html,body{{margin:0;height:100%;overflow:hidden}} #board{{position:fixed;inset:0}}</style></head>
<body><div id="board"></div>
<script type="application/json" id="nextpad-config">{cfg}</script>
<script src="{base}/static/board/assets-path.js?v={v}"></script>
<script type="module" src="{base}/static/board/board.js?v={v}"></script></body></html>"#, v = version(), cfg = s.cfg.to_string().replace("</", "<\\/"));
    let mut resp = Html(body).into_response();
    let csp = format!("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: {}; font-src 'self' data:; worker-src 'self' blob:; connect-src 'self'; object-src 'none'; form-action 'self'; frame-ancestors {}; base-uri 'none'", web::moodle_origins(state), state.config.frame_ancestors);
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&csp) { h.insert(header::CONTENT_SECURITY_POLICY, v); }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

fn export(_state: &AppState, _pad: &str, _format: &str, sp: Sprache) -> Response {
    web::error_page(sp, StatusCode::CONFLICT, texte::t(sp, "zeichenbrett.export_formate"))
}

async fn static_board(Path(path): Path<String>) -> Response {
    if path.contains("..") { return StatusCode::NOT_FOUND.into_response(); }
    let Some(f) = BOARD.get_file(&path) else { return StatusCode::NOT_FOUND.into_response() };
    let ct = match path.rsplit('.').next().unwrap_or("") {
        "js" => "text/javascript; charset=utf-8", "css" => "text/css; charset=utf-8", "woff2" => "font/woff2", "woff" => "font/woff",
        "ttf" => "font/ttf", "json" => "application/json", "svg" => "image/svg+xml", "png" => "image/png", _ => "application/octet-stream",
    };
    // Teile tragen einen Hash im Namen; board.js/css werden per ?v= versioniert
    ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "public, max-age=31536000, immutable"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], f.contents()).into_response()
}
