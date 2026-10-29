//! Erscheinungsbild: Name, Logo, Startbild und Farben, von Admins im Browser einstellbar.
//! Admin ist, wer über Moodle angemeldet ist und dort Administrator ist oder in NEXTPAD_ADMINS steht.
//! Einstellungen liegen als JSON in der Datenbank, hochgeladene Bilder im Datenverzeichnis.
//! Ohne Einstellung gilt die Vorgabe des Servers (NEXTPAD_THEME_DIR, NEXTPAD_BACKGROUND_CREDIT).
use crate::texte::{t, tf, Sprache};
use crate::sperre::LeseSperre;
use crate::{web, AppState};
use axum::{
    extract::{Multipart, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

pub const ADMIN_JS: &str = include_str!("../static/admin.js");
const MAX_LOGO: usize = 2 << 20;
const MAX_BACKGROUND: usize = 6 << 20;

/// Woher ein Bild kommt.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Source { #[default] Vorgabe, Eigen, Keins }

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Look {
    pub product: String,
    pub tagline: String,
    pub institution: String,
    pub logo: Source,
    pub background: Source,
    pub credit: String,
    pub credit_url: String,
    /// Akzent (Kopflinie, Fokus) und drei Hausfarben; leer = Vorgabe
    pub accent: String,
    pub color1: String,
    pub color2: String,
    pub color3: String,
    /// Linien und Tönungen selbst festlegen statt ableiten
    pub custom_shades: bool,
    pub line1: String,
    pub line2: String,
    pub tint1: String,
    pub tint2: String,
    pub tint3: String,
    /// zählt bei jedem Speichern hoch, für Bildadressen
    pub version: u64,
}

pub type Looks = RwLock<Look>;

pub fn load(store: &crate::store::Store) -> Look {
    store.setting("look").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn get(state: &AppState) -> Look { state.look.lesen().clone() }

fn dir(state: &AppState) -> std::path::PathBuf { state.config.data_dir.join("erscheinungsbild") }

// Vorgaben [CD] wie in start.css
const D_ACCENT: &str = "#e60028";
const D_COLORS: [&str; 3] = ["#00afd7", "#64b432", "#ffeb00"];
const D_LINES: [&str; 2] = ["#007a96", "#467e23"];
const D_TINTS: [&str; 3] = ["#e0f5fa", "#ecf6e6", "#fff9b2"];
const D_LINES_DARK: [&str; 2] = ["#00afd7", "#64b432"];
const D_TINTS_DARK: [&str; 3] = ["#16424c", "#304423", "#565116"];
const D_ACCENT_DARK: &str = "#ff4d6a";
const WHITE: Rgb = (255, 255, 255);
const BLACK: Rgb = (0, 0, 0);
const DARK: Rgb = (0x1e, 0x1e, 0x1e);

impl Look {
    pub fn product(&self) -> &str { if self.product.is_empty() { "NextPad" } else { &self.product } }
    /// Eingetragener Untertitel (Inhalt, nicht übersetzt), sonst die Vorgabe in der Sprache der Seite
    pub fn tagline(&self, sp: Sprache) -> &str { if self.tagline.is_empty() { t(sp, "look.untertitel") } else { &self.tagline } }
    pub fn institution(&self) -> &str { if self.institution.is_empty() { "Hochschule Düsseldorf" } else { &self.institution } }
    fn accent(&self) -> &str { or(&self.accent, D_ACCENT) }
    fn color(&self, i: usize) -> &str { or([&self.color1, &self.color2, &self.color3][i], D_COLORS[i]) }
    fn colors_changed(&self) -> bool { !(self.accent.is_empty() && self.color1.is_empty() && self.color2.is_empty() && self.color3.is_empty() && !self.custom_shades) }

    /// Farbwerte hell und dunkel, abgeleitet aus Akzent und Hausfarben.
    pub fn palette(&self) -> Palette {
        let same = |v: &str, d: &str| v.eq_ignore_ascii_case(d);
        let own = |v: &str| if self.custom_shades { parse(v) } else { None };
        let mut p = Palette { accent: self.accent().into(), accent_dark: D_ACCENT_DARK.into(), colors: D_COLORS.map(String::from), lines: D_LINES.map(String::from),
            tints: D_TINTS.map(String::from), lines_dark: D_LINES_DARK.map(String::from), tints_dark: D_TINTS_DARK.map(String::from) };
        if let Some(a) = parse(self.accent()).filter(|_| !same(self.accent(), D_ACCENT)) { p.accent_dark = hex(lift(a, DARK, 4.5)); }
        #[allow(clippy::needless_range_loop)] // i greift auf vier gleich lange Felder zugleich zu
        for i in 0..3 {
            p.colors[i] = self.color(i).into();
            let Some(c) = parse(self.color(i)) else { continue };
            if !same(self.color(i), D_COLORS[i]) {
                if i < 2 { p.lines[i] = hex(darken(c)); p.lines_dark[i] = hex(lift(c, DARK, 4.5)); }
                p.tints[i] = hex(mix(WHITE, c, if i == 2 { 0.30 } else { 0.12 }));
                p.tints_dark[i] = hex(mix(DARK, c, 0.25));
            }
        }
        for (i, v) in [&self.line1, &self.line2].into_iter().enumerate() { if let Some(c) = own(v) { p.lines[i] = hex(c); } }
        for (i, v) in [&self.tint1, &self.tint2, &self.tint3].into_iter().enumerate() { if let Some(c) = own(v) { p.tints[i] = hex(c); } }
        p
    }

    /// Hinweise zu Kontrasten, die WCAG 2.2 nicht erreichen.
    pub fn warnings(&self, sp: Sprache) -> Vec<String> {
        let p = self.palette();
        let mut w = Vec::new();
        let r = |a: &str, b: Rgb| parse(a).map(|a| ratio(a, b)).unwrap_or(21.0);
        let farbe = |i: usize| tf(sp, "look.farbe_n", &[("n", &(i + 1).to_string())]);
        if r(&p.accent, WHITE) < 3.0 { w.push(tf(sp, "look.w.akzent", &[("r", &zahl(sp, r(&p.accent, WHITE)))])); }
        for i in 0..2 {
            if r(&p.lines[i], WHITE) < 3.0 { w.push(tf(sp, "look.w.linie", &[("r", &zahl(sp, r(&p.lines[i], WHITE))), ("farbe", &farbe(i))])); }
        }
        for i in 0..3 {
            if r(&p.tints[i], BLACK) < 4.5 { w.push(tf(sp, "look.w.toenung", &[("r", &zahl(sp, r(&p.tints[i], BLACK))), ("farbe", &farbe(i))])); }
        }
        w
    }

    /// Stilangaben für den Seitenkopf; leer, solange die Farben der Vorgabe gelten.
    pub fn css(&self) -> String {
        if !self.colors_changed() { return String::new(); }
        let p = self.palette();
        format!(r#"<style>:root,.theme-hsd{{--hsd-red:{a};--primary:{a};--focus-ring:3px solid {a};--hsd-blue:{c0};--hsd-green:{c1};--hsd-yellow:{c2};--line-blue:{l0};--line-green:{l1};--tint-blue:{t0};--tint-green:{t1};--tint-yellow:{t2}}}
@media (prefers-color-scheme: dark){{:root{{--focus-ring:3px solid {ad};--line-blue:{ld0};--line-green:{ld1};--tint-blue:{td0};--tint-green:{td1};--tint-yellow:{td2}}}}}
html[data-theme="dark"] .theme-hsd{{--primary:{ad};--line-blue:{ld0};--tint-blue:{td0}}}</style>"#,
            a = p.accent, ad = p.accent_dark, c0 = p.colors[0], c1 = p.colors[1], c2 = p.colors[2], l0 = p.lines[0], l1 = p.lines[1], t0 = p.tints[0], t1 = p.tints[1], t2 = p.tints[2],
            ld0 = p.lines_dark[0], ld1 = p.lines_dark[1], td0 = p.tints_dark[0], td1 = p.tints_dark[1], td2 = p.tints_dark[2])
    }
}

pub struct Palette { pub accent: String, pub accent_dark: String, pub colors: [String; 3], pub lines: [String; 2], pub tints: [String; 3], pub lines_dark: [String; 2], pub tints_dark: [String; 3] }

fn or<'a>(v: &'a str, d: &'a str) -> &'a str { if v.is_empty() { d } else { v } }

type Rgb = (u8, u8, u8);
fn parse(s: &str) -> Option<Rgb> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
fn hex(c: Rgb) -> String { format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2) }
fn lum(c: Rgb) -> f64 {
    let f = |v: u8| { let v = v as f64 / 255.0; if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } };
    0.2126 * f(c.0) + 0.7152 * f(c.1) + 0.0722 * f(c.2)
}
pub fn ratio(a: Rgb, b: Rgb) -> f64 { let (x, y) = (lum(a), lum(b)); (x.max(y) + 0.05) / (x.min(y) + 0.05) }
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}
/// Linie auf Weiß: so wenig abdunkeln wie nötig für 4,5:1.
fn darken(c: Rgb) -> Rgb { (0..=100).map(|s| mix(c, BLACK, s as f64 / 100.0)).find(|m| ratio(*m, WHITE) >= 4.5).unwrap_or(BLACK) }
/// Auf dunklem Grund: so wenig aufhellen wie nötig.
fn lift(c: Rgb, bg: Rgb, min: f64) -> Rgb { (0..=100).map(|s| mix(c, WHITE, s as f64 / 100.0)).find(|m| ratio(*m, bg) >= min).unwrap_or(WHITE) }

/// Bildart aus den ersten Bytes; SVG nur ohne Skript und Ereignisattribute.
fn sniff(b: &[u8], svg_ok: bool) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") { return Some("image/png"); }
    if b.starts_with(&[0xff, 0xd8, 0xff]) { return Some("image/jpeg"); }
    if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" { return Some("image/webp"); }
    if svg_ok {
        let s = std::str::from_utf8(b).ok()?.to_ascii_lowercase();
        let head = s.trim_start_matches('\u{feff}').trim_start();
        let svg = head.starts_with("<svg") || ((head.starts_with("<?xml") || head.starts_with("<!--")) && s.contains("<svg"));
        let risky = ["<script", "javascript:", "<foreignobject", "<!entity"].iter().any(|t| s.contains(t))
            || s.match_indices("on").any(|(i, _)| i > 0 && matches!(s.as_bytes()[i - 1], b' ' | b'\n' | b'\r' | b'\t' | b'/' | b'"' | b'\'')
                && s[i + 2..].trim_start_matches(|c: char| c.is_ascii_alphabetic()).trim_start().starts_with('='));
        if svg && !risky { return Some("image/svg+xml"); }
    }
    None
}

/// Logo nach Einstellung: eigenes Bild, Vorgabe aus NEXTPAD_THEME_DIR oder keins.
pub fn logo(state: &AppState) -> Option<(Vec<u8>, &'static str)> { image(state, get(state).logo, "logo", "logo.png", true) }
pub fn background(state: &AppState) -> Option<(Vec<u8>, &'static str)> { image(state, get(state).background, "hintergrund", "background.jpg", false) }
fn image(state: &AppState, src: Source, own: &str, default: &str, svg_ok: bool) -> Option<(Vec<u8>, &'static str)> {
    let b = match src {
        Source::Keins => return None,
        Source::Eigen => std::fs::read(dir(state).join(own)).ok()?,
        Source::Vorgabe => std::fs::read(state.config.theme_dir.as_ref()?.join(default)).ok()?,
    };
    let t = sniff(&b, svg_ok)?;
    Some((b, t))
}
pub fn has_logo(state: &AppState) -> bool { exists(state, get(state).logo, "logo", "logo.png") }
pub fn has_background(state: &AppState) -> bool { exists(state, get(state).background, "hintergrund", "background.jpg") }
fn exists(state: &AppState, src: Source, own: &str, default: &str) -> bool {
    match src {
        Source::Keins => false,
        Source::Eigen => dir(state).join(own).is_file(),
        Source::Vorgabe => state.config.theme_dir.as_ref().map(|d| d.join(default).is_file()).unwrap_or(false),
    }
}
fn default_exists(state: &AppState, name: &str) -> bool { state.config.theme_dir.as_ref().map(|d| d.join(name).is_file()).unwrap_or(false) }

/// Bildnachweis: eingestellt oder aus NEXTPAD_BACKGROUND_CREDIT.
pub fn credit(state: &AppState) -> Option<(String, Option<String>)> {
    let l = get(state);
    if l.background == Source::Eigen { return (!l.credit.is_empty()).then(|| (l.credit.clone(), Some(l.credit_url.clone()).filter(|u| !u.is_empty()))); }
    state.config.background_credit.clone().map(|c| (c, state.config.background_credit_url.clone()))
}

fn serve(img: Option<(Vec<u8>, &'static str)>) -> Response {
    match img {
        // SVG sicher ausliefern: direkt aufgerufen läuft darin nichts
        Some((b, t)) => ([(header::CONTENT_TYPE, t), (header::CACHE_CONTROL, "public, max-age=86400"), (header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'; sandbox"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], b).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
pub async fn logo_file(State(state): State<Arc<AppState>>) -> Response { serve(logo(&state)) }
pub async fn background_file(State(state): State<Arc<AppState>>) -> Response { serve(background(&state)) }
pub async fn static_js() -> Response {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=86400")], ADMIN_JS).into_response()
}

// Verwaltungsseite

fn esc(s: &str) -> String { html_escape::encode_double_quoted_attribute(s).into_owned() }

/// Kontrastwert mit einer Nachkommastelle, deutsch mit Komma.
fn zahl(sp: Sprache, x: f64) -> String {
    let z = format!("{x:.1}");
    if sp == Sprache::De { z.replace('.', ",") } else { z }
}

fn denied(state: &AppState, sp: Sprache) -> Response {
    let base = &state.config.base_path;
    let content = format!(r#"<main id="inhalt" class="doc" tabindex="-1"><h1>{titel}</h1><p>{text}</p><p><a href="{base}/">{start}</a></p></main>"#,
        titel = t(sp, "look.titel"), text = t(sp, "look.nur_admins"), start = t(sp, "look.zur_startseite"));
    let mut r = web::start_page(state, sp, Some(PFAD), &format!("{} · NextPad", t(sp, "look.titel")), &content, "");
    *r.status_mut() = StatusCode::FORBIDDEN;
    r
}

/// Adresse der Seite ohne Basis-Pfad, für den Sprachumschalter
const PFAD: &str = "/admin/erscheinungsbild";

fn color_field(sp: Sprache, name: &str, label: &str, value: &str, default: &str, hint: &str) -> String {
    let shown = if value.is_empty() { default } else { value };
    format!(r##"<div class="color-field"><label class="field-label" for="f-{name}">{label}</label>
<div class="color-row"><input type="color" id="c-{name}" value="{shown}" aria-label="{waehlen}" data-for="f-{name}"><input type="text" id="f-{name}" name="{name}" value="{v}" placeholder="{default}" pattern="#[0-9a-fA-F]{{6}}" maxlength="7" spellcheck="false" autocomplete="off" aria-describedby="h-{name}"></div>
<p class="hint" id="h-{name}">{hint} {leer}</p></div>"##, v = esc(value),
        waehlen = esc(&tf(sp, "look.farbe_waehlen", &[("label", label)])), leer = tf(sp, "look.leer_vorgabe", &[("vorgabe", default)]))
}

fn swatches(sp: Sprache, p: &Palette) -> String {
    // Linien gegen Weiß, Tönungen gegen schwarzen Text
    let sw = |c: &str, label: String, toenung: bool| {
        let (against, what) = if toenung { (BLACK, t(sp, "look.text_darauf")) } else { (WHITE, t(sp, "look.auf_weiss")) };
        let r = parse(c).map(|x| format!("{}:1", zahl(sp, ratio(x, against)))).unwrap_or_default();
        format!(r#"<li><span class="swatch" style="background:{c}"></span><span>{label}<br><code>{c}</code> · {r} {what}</span></li>"#)
    };
    let n = |k: &'static str, i: usize| tf(sp, k, &[("n", &i.to_string())]);
    let items = [sw(&p.accent, t(sp, "look.akzent").to_string(), false), sw(&p.lines[0], n("look.linie_n", 1), false), sw(&p.lines[1], n("look.linie_n", 2), false),
        sw(&p.tints[0], n("look.toenung_n", 1), true), sw(&p.tints[1], n("look.toenung_n", 2), true), sw(&p.tints[2], n("look.toenung_n", 3), true)].concat();
    format!(r#"<ul class="swatches" aria-label="{}">{items}</ul>"#, t(sp, "look.verwendet"))
}

/// Auswahl Vorgabe / eigenes / keins; `eigen` und `keins` sind Schlüssel der Beschriftung.
fn radios(sp: Sprache, name: &str, cur: Source, has_default: bool, has_own: bool, eigen: &'static str, keins: &'static str) -> String {
    let opt = |v: &str, label: &str, on: bool, enabled: bool| {
        if !enabled { return String::new(); }
        format!(r#"<label class="choice"><input type="radio" name="{name}" value="{v}"{}> {label}</label>"#, if on { " checked" } else { "" })
    };
    let cur = match cur { Source::Eigen if !has_own => Source::Vorgabe, c => c };
    [opt("vorgabe", t(sp, if has_default { "look.r.vorgabe" } else { "look.r.vorgabe_fehlt" }), cur == Source::Vorgabe, true),
     opt("eigen", t(sp, eigen), cur == Source::Eigen, has_own),
     opt("keins", t(sp, keins), cur == Source::Keins, true)].concat()
}

fn form(state: &AppState, sp: Sprache, l: &Look, notice: &str) -> String {
    let base = &state.config.base_path;
    let p = l.palette();
    let has_own_logo = dir(state).join("logo").is_file();
    let has_own_bg = dir(state).join("hintergrund").is_file();
    let logo_prev = if has_logo(state) { format!(r#"<img class="preview-logo" src="{base}/theme/logo.png?v={v}" alt="{alt}">"#, v = l.version, alt = t(sp, "look.logo_aktuell")) } else { format!(r#"<p class="hint">{}</p>"#, t(sp, "look.logo_keins")) };
    let bg_prev = if has_background(state) { format!(r#"<img class="preview-bg" src="{base}/theme/background.jpg?v={v}" alt="{alt}">"#, v = l.version, alt = t(sp, "look.bild_aktuell")) } else { format!(r#"<p class="hint">{}</p>"#, t(sp, "look.bild_keins")) };
    let warnings: String = l.warnings(sp).iter().map(|w| format!("<li>{}</li>", html_escape::encode_text(w))).collect();
    let warnings = if warnings.is_empty() { String::new() } else { format!(r#"<div class="notice warn" role="status"><p><strong>{}</strong></p><ul>{warnings}</ul></div>"#, t(sp, "look.kontrast")) };
    let checked = if l.custom_shades { " checked" } else { "" };
    let farbe = |k: &'static str, i: usize| tf(sp, k, &[("n", &i.to_string())]);
    format!(r##"<main id="inhalt" class="doc admin" tabindex="-1"><p><a href="{base}/">← {start}</a></p>
<p class="kicker">{verwaltung}</p><h1>{titel}</h1>
<p>{einleitung}</p>
{notice}{warnings}
<form method="post" action="{base}/admin/erscheinungsbild" enctype="multipart/form-data" class="admin-form">
<fieldset><legend>{name}</legend>
<label class="field-label" for="f-product">{name_kopf}</label><input type="text" id="f-product" name="product" maxlength="40" value="{product}" placeholder="NextPad">
<label class="field-label" for="f-tagline">{untertitel_feld}</label><input type="text" id="f-tagline" name="tagline" maxlength="80" value="{tagline}" placeholder="{untertitel}">
<label class="field-label" for="f-institution">{einrichtung}</label><input type="text" id="f-institution" name="institution" maxlength="80" value="{institution}" placeholder="Hochschule Düsseldorf" aria-describedby="h-institution">
<p class="hint" id="h-institution">{einrichtung_hinweis}</p>
</fieldset>
<fieldset><legend>{logo_titel}</legend>
{logo_prev}
<div class="choices">{logo_radios}</div>
<label class="field-label" for="f-logo">{logo_hoch}</label><input type="file" id="f-logo" name="logo_file" accept="image/png,image/jpeg,image/webp,image/svg+xml" aria-describedby="h-logo">
<p class="hint" id="h-logo">{logo_hinweis}</p>
</fieldset>
<fieldset><legend>{bild_titel}</legend>
{bg_prev}
<div class="choices">{bg_radios}</div>
<label class="field-label" for="f-bg">{bild_hoch}</label><input type="file" id="f-bg" name="background_file" accept="image/png,image/jpeg,image/webp" aria-describedby="h-bg">
<p class="hint" id="h-bg">{bild_hinweis}</p>
<label class="field-label" for="f-credit">{nachweis}</label><input type="text" id="f-credit" name="credit" maxlength="200" value="{credit}" placeholder="{nachweis_platzhalter}">
<label class="field-label" for="f-credit-url">{nachweis_link}</label><input type="url" id="f-credit-url" name="credit_url" maxlength="300" value="{credit_url}" placeholder="https://…">
</fieldset>
<fieldset><legend>{farben}</legend>
{accent}{c1}{c2}{c3}
<label class="choice"><input type="checkbox" name="custom_shades" id="f-custom" value="1"{checked}> {selbst}</label>
<div class="shades" id="shades">{l1}{l2}{t1}{t2}{t3}</div>
<h3>{vorschau}</h3>{swatches}
</fieldset>
<p><button type="submit" class="btn btn-primary">{speichern}</button></p>
</form>
<form method="post" action="{base}/admin/erscheinungsbild/vorgabe" class="reset-form"><button type="submit" class="btn btn-secondary">{zuruecksetzen}</button></form>
</main>"##,
        start = t(sp, "look.zur_startseite"), verwaltung = t(sp, "look.verwaltung"), titel = t(sp, "look.titel"), einleitung = t(sp, "look.einleitung"),
        name = t(sp, "look.name"), name_kopf = t(sp, "look.name_kopf"), untertitel_feld = t(sp, "look.untertitel_feld"), untertitel = t(sp, "look.untertitel"),
        einrichtung = t(sp, "look.einrichtung"), einrichtung_hinweis = t(sp, "look.einrichtung_hinweis"),
        logo_titel = t(sp, "look.logo_titel"), logo_hoch = t(sp, "look.logo_hoch"), logo_hinweis = t(sp, "look.logo_hinweis"),
        bild_titel = t(sp, "look.bild_titel"), bild_hoch = t(sp, "look.bild_hoch"), bild_hinweis = t(sp, "look.bild_hinweis"),
        nachweis = t(sp, "look.nachweis"), nachweis_platzhalter = t(sp, "look.nachweis_platzhalter"), nachweis_link = t(sp, "look.nachweis_link"),
        farben = t(sp, "look.farben"), selbst = t(sp, "look.selbst"), vorschau = t(sp, "look.vorschau"), speichern = t(sp, "look.speichern"), zuruecksetzen = t(sp, "look.zuruecksetzen"),
        product = esc(&l.product), tagline = esc(&l.tagline), institution = esc(&l.institution), credit = esc(&l.credit), credit_url = esc(&l.credit_url),
        logo_radios = radios(sp, "logo", l.logo, default_exists(state, "logo.png"), has_own_logo, "look.r.eigenes_logo", "look.r.kein_logo"),
        bg_radios = radios(sp, "background", l.background, default_exists(state, "background.jpg"), has_own_bg, "look.r.eigenes_bild", "look.r.kein_bild"),
        accent = color_field(sp, "accent", t(sp, "look.akzentfarbe"), &l.accent, D_ACCENT, t(sp, "look.h.akzent")),
        c1 = color_field(sp, "color1", &farbe("look.farbe_n", 1), &l.color1, D_COLORS[0], t(sp, "look.h.farbe1")),
        c2 = color_field(sp, "color2", &farbe("look.farbe_n", 2), &l.color2, D_COLORS[1], t(sp, "look.h.farbe2")),
        c3 = color_field(sp, "color3", &farbe("look.farbe_n", 3), &l.color3, D_COLORS[2], t(sp, "look.h.farbe3")),
        l1 = color_field(sp, "line1", &farbe("look.linie_n", 1), &l.line1, &p.lines[0], t(sp, "look.h.linie1")),
        l2 = color_field(sp, "line2", &farbe("look.linie_n", 2), &l.line2, &p.lines[1], t(sp, "look.h.linie2")),
        t1 = color_field(sp, "tint1", &farbe("look.toenung_n", 1), &l.tint1, &p.tints[0], t(sp, "look.h.toenung1")),
        t2 = color_field(sp, "tint2", &farbe("look.toenung_n", 2), &l.tint2, &p.tints[1], t(sp, "look.h.toenung2")),
        t3 = color_field(sp, "tint3", &farbe("look.toenung_n", 3), &l.tint3, &p.tints[2], t(sp, "look.h.toenung3")),
        swatches = swatches(sp, &p))
}

fn render(state: &AppState, sp: Sprache, l: &Look, notice: &str, status: StatusCode) -> Response {
    let base = &state.config.base_path;
    let scripts = format!(r#"<script src="{base}/static/admin.js?v={}"></script>"#, env!("CARGO_PKG_VERSION"));
    let mut r = web::start_page(state, sp, Some(PFAD), &format!("{} · NextPad", t(sp, "look.titel")), &form(state, sp, l, notice), &scripts);
    *r.status_mut() = status;
    r
}

pub async fn page(State(state): State<Arc<AppState>>, headers: HeaderMap, q: axum::extract::Query<std::collections::HashMap<String, String>>) -> Response {
    let sp = Sprache::aus_anfrage(&headers);
    if web::login_admin(&state, &headers).is_none() { return denied(&state, sp); }
    let notice = if q.contains_key("gespeichert") { format!(r#"<div class="notice ok" role="status"><p>{}</p></div>"#, t(sp, "look.gespeichert")) } else { String::new() };
    render(&state, sp, &get(&state), &notice, StatusCode::OK)
}

fn clean(s: &str, max: usize) -> String { s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string() }

pub async fn save(State(state): State<Arc<AppState>>, headers: HeaderMap, mut mp: Multipart) -> Response {
    if !web::origin_ok(&state, &headers) { return StatusCode::FORBIDDEN.into_response(); }
    let sp = Sprache::aus_anfrage(&headers);
    let Some(admin) = web::login_admin(&state, &headers) else { return denied(&state, sp) };
    let old = get(&state);
    let mut l = Look { version: old.version + 1, ..Look::default() };
    let (mut logo_up, mut bg_up) = (None, None);
    let mut errors: Vec<String> = Vec::new();
    loop {
        let field = match mp.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(_) => { errors.push(t(sp, "look.e.formular").into()); break; }
        };
        let name = field.name().unwrap_or("").to_string();
        let Ok(bytes) = field.bytes().await else { errors.push(t(sp, "look.e.formular").into()); break };
        match name.as_str() {
            "logo_file" if !bytes.is_empty() => logo_up = Some(bytes),
            "background_file" if !bytes.is_empty() => bg_up = Some(bytes),
            "logo_file" | "background_file" => {}
            _ => {
                let v = String::from_utf8_lossy(&bytes).to_string();
                let src = |v: &str| match v { "eigen" => Source::Eigen, "keins" => Source::Keins, _ => Source::Vorgabe };
                match name.as_str() {
                    "product" => l.product = clean(&v, 40),
                    "tagline" => l.tagline = clean(&v, 80),
                    "institution" => l.institution = clean(&v, 80),
                    "credit" => l.credit = clean(&v, 200),
                    "credit_url" => l.credit_url = clean(&v, 300),
                    "logo" => l.logo = src(&v),
                    "background" => l.background = src(&v),
                    "custom_shades" => l.custom_shades = v == "1",
                    "accent" | "color1" | "color2" | "color3" | "line1" | "line2" | "tint1" | "tint2" | "tint3" => {
                        let v = v.trim().to_ascii_lowercase();
                        if !v.is_empty() && parse(&v).is_none() { errors.push(tf(sp, "look.e.farbe", &[("wert", &clean(&v, 20))])); }
                        let slot = match name.as_str() { "accent" => &mut l.accent, "color1" => &mut l.color1, "color2" => &mut l.color2, "color3" => &mut l.color3,
                            "line1" => &mut l.line1, "line2" => &mut l.line2, "tint1" => &mut l.tint1, "tint2" => &mut l.tint2, _ => &mut l.tint3 };
                        *slot = v;
                    }
                    _ => {}
                }
            }
        }
    }
    if !l.credit_url.is_empty() && !l.credit_url.starts_with("https://") { errors.push(t(sp, "look.e.link").into()); }
    if let Some(b) = &logo_up {
        if b.len() > MAX_LOGO { errors.push(t(sp, "look.e.logo_gross").into()); }
        else if sniff(b, true).is_none() { errors.push(t(sp, "look.e.logo_art").into()); }
        else { l.logo = Source::Eigen; }
    }
    if let Some(b) = &bg_up {
        if b.len() > MAX_BACKGROUND { errors.push(t(sp, "look.e.bild_gross").into()); }
        else if sniff(b, false).is_none() { errors.push(t(sp, "look.e.bild_art").into()); }
        else { l.background = Source::Eigen; }
    }
    if l.logo == Source::Eigen && logo_up.is_none() && !dir(&state).join("logo").is_file() { l.logo = Source::Vorgabe; }
    if l.background == Source::Eigen && bg_up.is_none() && !dir(&state).join("hintergrund").is_file() { l.background = Source::Vorgabe; }
    if !errors.is_empty() {
        let items: String = errors.iter().map(|e| format!("<li>{}</li>", html_escape::encode_text(e))).collect();
        let notice = format!(r#"<div class="notice error" role="alert"><p><strong>{}</strong></p><ul>{items}</ul></div>"#, t(sp, "look.nicht_gespeichert"));
        return render(&state, sp, &Look { logo: old.logo, background: old.background, ..l }, &notice, StatusCode::BAD_REQUEST);
    }
    let d = dir(&state);
    let written = std::fs::create_dir_all(&d).and_then(|_| {
        if let Some(b) = &logo_up { write_atomic(&d.join("logo"), b)?; }
        if let Some(b) = &bg_up { write_atomic(&d.join("hintergrund"), b)?; }
        Ok(())
    });
    if let Err(e) = written { tracing::error!("Erscheinungsbild: Bild speichern: {e}"); return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    store(&state, l, &admin.name)
}

fn write_atomic(path: &std::path::Path, b: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("neu");
    std::fs::write(&tmp, b)?;
    std::fs::rename(tmp, path)
}

fn store(state: &AppState, l: Look, who: &str) -> Response {
    let json = serde_json::to_string(&l).unwrap_or_default();
    if let Err(e) = state.store.set_setting("look", &json) { tracing::error!("Erscheinungsbild speichern: {e}"); return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    *state.look.schreiben() = l;
    tracing::info!("Erscheinungsbild geändert von {who}");
    (StatusCode::SEE_OTHER, [(header::LOCATION, format!("{}/admin/erscheinungsbild?gespeichert=1", state.config.base_path))]).into_response()
}

/// Alles zurück auf die Vorgabe; hochgeladene Bilder bleiben liegen und sind wieder wählbar.
pub async fn reset(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !web::origin_ok(&state, &headers) { return StatusCode::FORBIDDEN.into_response(); }
    let Some(admin) = web::login_admin(&state, &headers) else { return denied(&state, Sprache::aus_anfrage(&headers)) };
    let version = get(&state).version + 1;
    store(&state, Look { version, ..Look::default() }, &admin.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_unchanged() {
        let l = Look::default();
        assert!(l.css().is_empty());
        let p = l.palette();
        assert_eq!(p.lines[0], "#007a96");
        assert_eq!(p.tints[2], "#fff9b2");
        assert!(l.warnings(Sprache::De).is_empty());
        assert_eq!(l.tagline(Sprache::En), "Writing together at HSD");
    }
    #[test]
    fn derived_lines_reach_contrast() {
        let l = Look { color1: "#7fd4ff".into(), accent: "#ffcc00".into(), ..Look::default() };
        let p = l.palette();
        assert!(ratio(parse(&p.lines[0]).unwrap(), WHITE) >= 4.5);
        assert!(ratio(parse(&p.lines_dark[0]).unwrap(), DARK) >= 4.5);
        assert!(l.warnings(Sprache::De).iter().any(|w| w.starts_with("Akzentfarbe: nur 1,") && w.ends_with("mindestens 3:1.")));
        assert!(l.warnings(Sprache::En).iter().any(|w| w.starts_with("Accent colour: only 1.")));
        assert!(l.css().contains("--hsd-red:#ffcc00"));
    }
    #[test]
    fn svg_with_script_refused() {
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'><rect/></svg>", true), Some("image/svg+xml"));
        assert_eq!(sniff(b"<svg><script>alert(1)</script></svg>", true), None);
        assert_eq!(sniff(b"<svg onload = 'x'></svg>", true), None);
        assert_eq!(sniff(b"<svg\nonload='x'></svg>", true), None);
        assert_eq!(sniff(b"<svg><rect/></svg>", false), None);
    }
}
