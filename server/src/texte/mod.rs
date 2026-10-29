//! Textkatalog: Jeder sichtbare Text hat einen Schlüssel, je Sprache steht er in einem Katalog
//! (`de.rs`, `en.rs`). Addons bringen ihre Texte selbst mit (`Addon::texte`).
//!
//! Sprache einer Anfrage, in dieser Reihenfolge: eigene Wahl (Cookie `nextpad-lang`), Sprache aus
//! Moodle beim LTI-Start (Cookie `nextpad-lang-moodle`), Browsersprache (`Accept-Language`), Deutsch.
mod de;
mod en;

use axum::http::{header, HeaderMap};
use std::collections::HashMap;

pub const WAHL_COOKIE: &str = "nextpad-lang";
pub const MOODLE_COOKIE: &str = "nextpad-lang-moodle";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprache { De, En }

impl Sprache {
    pub fn kuerzel(self) -> &'static str { match self { Sprache::De => "de", Sprache::En => "en" } }

    /// „de“, „de-DE“, „en_US“ … nach der Hauptsprache; andere Sprachen: keine.
    pub fn aus_kuerzel(s: &str) -> Option<Self> {
        let haupt = s.trim().split(['-', '_']).next()?.to_ascii_lowercase();
        match haupt.as_str() { "de" => Some(Sprache::De), "en" => Some(Sprache::En), _ => None }
    }

    /// Nur die eigene Wahl über den Umschalter, sonst keine.
    pub fn gewaehlt(headers: &HeaderMap) -> Option<Self> { cookie(headers, WAHL_COOKIE) }

    pub fn aus_anfrage(headers: &HeaderMap) -> Self {
        cookie(headers, WAHL_COOKIE)
            .or_else(|| cookie(headers, MOODLE_COOKIE))
            .or_else(|| headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).and_then(aus_accept_language))
            .unwrap_or(Sprache::De)
    }
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<Sprache> {
    crate::web::cookie_values(headers, name).first().and_then(|v| Sprache::aus_kuerzel(v))
}

/// Erste unterstützte Sprache nach Gewicht (`q`); `q=0` heißt „nicht gewünscht“.
fn aus_accept_language(v: &str) -> Option<Sprache> {
    let mut wuensche: Vec<(f32, Sprache)> = v.split(',').filter_map(|teil| {
        let mut it = teil.split(';');
        let sprache = Sprache::aus_kuerzel(it.next()?)?;
        let q = it.find_map(|p| p.trim().strip_prefix("q=")).map(|q| q.trim().parse().unwrap_or(0.0)).unwrap_or(1.0);
        (q > 0.0).then_some((q, sprache))
    }).collect();
    wuensche.sort_by(|a, b| b.0.total_cmp(&a.0));
    wuensche.first().map(|w| w.1)
}

/// Schlüssel und Text
pub type Texte = &'static [(&'static str, &'static str)];

type Katalog = HashMap<(Sprache, &'static str), &'static str>;

fn katalog() -> &'static Katalog {
    static K: std::sync::OnceLock<Katalog> = std::sync::OnceLock::new();
    K.get_or_init(|| {
        let mut k = Katalog::new();
        for (sprache, texte) in quellen() {
            for (schluessel, text) in texte { k.insert((sprache, *schluessel), *text); }
        }
        k
    })
}

/// Alle Kataloge: Kern und Addons, je Sprache.
fn quellen() -> Vec<(Sprache, Texte)> {
    let mut q = vec![(Sprache::De, de::TEXTE), (Sprache::En, en::TEXTE)];
    for a in crate::addons::ALLE { q.push((Sprache::De, a.texte.0)); q.push((Sprache::En, a.texte.1)); }
    q
}

/// Text zum Schlüssel. Fehlt er in der Sprache, gilt Deutsch; fehlt er ganz, erscheint der Schlüssel.
pub fn t(sprache: Sprache, schluessel: &'static str) -> &'static str {
    let k = katalog();
    k.get(&(sprache, schluessel)).or_else(|| k.get(&(Sprache::De, schluessel))).copied().unwrap_or_else(|| {
        tracing::warn!("Text fehlt im Katalog: {schluessel}");
        schluessel
    })
}

/// Text mit Platzhaltern `{name}`, in einem Durchgang: Klammern in eingesetzten Werten bleiben, wie sie sind.
/// Unbekannte Platzhalter bleiben stehen.
pub fn tf(sprache: Sprache, schluessel: &'static str, werte: &[(&str, &str)]) -> String {
    let vorlage = t(sprache, schluessel);
    let mut out = String::with_capacity(vorlage.len());
    let mut rest = vorlage;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let nach = &rest[i + 1..];
        let treffer = nach.find('}').and_then(|j| werte.iter().find(|(n, _)| *n == &nach[..j]).map(|(_, w)| (*w, j)));
        match treffer {
            Some((w, j)) => { out.push_str(w); rest = &nach[j + 1..]; }
            None => { out.push('{'); rest = nach; }
        }
    }
    out.push_str(rest);
    out
}

/// Texte mit diesem Präfix als JSON für ein Skript (ohne Präfix), sicher in `<script type="application/json">`.
pub fn fuer_skript(sprache: Sprache, praefix: &str) -> String {
    texte_mit(sprache, praefix).to_string().replace('<', "\\u003c")
}

/// Alle Texte mit `praefix` als JSON-Objekt, Schlüssel ohne Präfix (etwa für einen Konfigurationsblock).
pub fn texte_mit(sprache: Sprache, praefix: &str) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (_, texte) in quellen().into_iter().filter(|q| q.0 == Sprache::De) {
        for (schluessel, _) in texte.iter().filter(|e| e.0.starts_with(praefix)) {
            m.insert(schluessel[praefix.len()..].to_string(), t(sprache, schluessel).into());
        }
    }
    serde_json::Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn kopf(paare: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (n, v) in paare { h.append(*n, v.parse().unwrap()); }
        h
    }

    fn platzhalter(s: &str) -> BTreeSet<String> {
        s.split('{').skip(1).filter_map(|r| r.split_once('}').map(|(n, _)| n.to_string())).collect()
    }

    #[test]
    fn beide_sprachen_haben_dieselben_schluessel_und_platzhalter() {
        let q = quellen();
        for paar in q.chunks(2) {
            let (de, en) = (paar[0].1, paar[1].1);
            let sd: BTreeSet<_> = de.iter().map(|e| e.0).collect();
            let se: BTreeSet<_> = en.iter().map(|e| e.0).collect();
            assert_eq!(sd.len(), de.len(), "doppelter Schlüssel im deutschen Katalog");
            assert_eq!(se.len(), en.len(), "doppelter Schlüssel im englischen Katalog");
            assert_eq!(sd, se, "Schlüssel unterscheiden sich");
            for (k, d) in de {
                let e = en.iter().find(|x| x.0 == *k).unwrap().1;
                assert_eq!(platzhalter(d), platzhalter(e), "Platzhalter in {k}");
                assert!(!d.trim().is_empty() && !e.trim().is_empty(), "leerer Text: {k}");
            }
        }
        let alle: Vec<_> = q.iter().filter(|x| x.0 == Sprache::De).flat_map(|x| x.1.iter().map(|e| e.0)).collect();
        assert_eq!(alle.iter().collect::<BTreeSet<_>>().len(), alle.len(), "Schlüssel in Kern und Addons doppelt");
    }

    #[test]
    fn reihenfolge_der_sprachwahl() {
        assert_eq!(Sprache::aus_anfrage(&HeaderMap::new()), Sprache::De);
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "en-US,en;q=0.9,de;q=0.8")])), Sprache::En);
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "fr-FR,fr;q=0.9,de;q=0.5,en;q=0.4")])), Sprache::De);
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "fr,it")])), Sprache::De);
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "en;q=0,de;q=0.1")])), Sprache::De);
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "de;q=0.2, en")])), Sprache::En);
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "*")])), Sprache::De);
        // Moodle vor Browser, eigene Wahl vor beidem
        let m = [("accept-language", "de-DE"), ("cookie", "nextpad-lang-moodle=en_us")];
        assert_eq!(Sprache::aus_anfrage(&kopf(&m)), Sprache::En);
        let w = [("accept-language", "en"), ("cookie", "nextpad-lang-moodle=en; nextpad-lang=de")];
        assert_eq!(Sprache::aus_anfrage(&kopf(&w)), Sprache::De);
        // unbrauchbarer Cookie zählt nicht
        assert_eq!(Sprache::aus_anfrage(&kopf(&[("accept-language", "en"), ("cookie", "nextpad-lang=xx")])), Sprache::En);
    }

    #[test]
    fn texte_und_platzhalter() {
        assert_eq!(t(Sprache::En, "fehler.titel.404"), "Not found");
        assert_eq!(t(Sprache::De, "fehler.titel.404"), "Nicht gefunden");
        assert_eq!(t(Sprache::En, "gibt.es.nicht"), "gibt.es.nicht");
        assert!(tf(Sprache::En, "addon.nicht_aktiv", &[("name", "Quiz")]).contains("“Quiz”"));
        // Werte werden nicht noch einmal ersetzt, egal in welcher Reihenfolge sie stehen
        assert_eq!(tf(Sprache::De, "mail.betreff", &[("titel", "Plan {datum}"), ("datum", "1.1.")]), "NextPad: Pad „Plan {datum}“ wird am 1.1. gelöscht");
        assert_eq!(tf(Sprache::De, "lti.addon_aus", &[]), t(Sprache::De, "lti.addon_aus"));
    }
}
