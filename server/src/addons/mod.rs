//! Addons: Pad-Typen neben dem Text-Pad. Jedes Addon beschreibt sich in einem `Addon`;
//! der Kern kennt nur dieses Verzeichnis. Eingeschaltet wird je Instanz mit `NEXTPAD_ADDONS`.
//! Die Tabellen aller Addons legt der Speicher immer an und räumt sie beim Löschen eines Pads mit weg,
//! auch wenn ein Addon ausgeschaltet ist; ausgeschaltet heißt: keine Kachel, keine Routen, keine Hilfe, kein Moodle-Typ.
use crate::{store::{Store, Zusatz}, texte::{self, Sprache}, AppState};
use axum::{http::{HeaderMap, StatusCode}, response::Response, Router};
use std::sync::Arc;

pub mod ideen;
pub mod quiz;
pub mod zeichenbrett;

/// Was der Kern einem Addon zum Anzeigen eines Pads mitgibt.
pub struct Seite<'a> {
    pub state: &'a AppState,
    pub headers: &'a HeaderMap,
    /// Kennung aus der Adresse (Pad-Name oder Leselink `r.…`)
    pub url_id: &'a str,
    pub pad_id: &'a str,
    /// freies Pad (Startseite) statt Moodle-Aktivität
    pub free: bool,
    /// Leselink, nur für Schreibende
    pub readonly_id: Option<&'a str>,
    /// Einstellungen für das Skript der Seite (wie beim Text-Pad)
    pub cfg: serde_json::Value,
}

/// Setzt beim ersten Start aus Moodle den Titel: (Speicher, Pad, Titel)
pub type TitelSetzen = fn(&Store, &str, &str) -> anyhow::Result<()>;

pub struct Addon {
    /// Kennung in `NEXTPAD_ADDONS`, in `/new?typ=` und als Moodle-Parameter `typ`
    pub kennung: &'static str,
    /// Wert in `pads.kind`
    pub art: &'static str,
    /// Schlüssel im Textkatalog des Addons
    pub name: &'static str,
    pub daten: Option<&'static Zusatz>,
    pub routen: fn() -> Router<Arc<AppState>>,
    pub seite: fn(Seite) -> Response,
    /// Export im Format der Adresse (`md`, `txt`, …); das Addon antwortet auch, wenn es ein Format nicht kann
    pub export: fn(&AppState, &str, &str, Sprache) -> Response,
    /// Kachel auf der Startseite (Schlüssel), `{base}` wird ersetzt
    pub kachel: &'static str,
    /// Satz in der Mail vor dem Löschen (Schlüssel): wie sich der Inhalt sichern lässt (Addons bekommen kein PDF)
    pub sichern: &'static str,
    /// Abschnitt der Hilfeseite (Schlüssel, HTML)
    pub hilfe: &'static str,
    /// Als Kursaktivität aus Moodle startbar; setzt beim ersten Start den Titel der Aktivität
    pub moodle: Option<TitelSetzen>,
    /// Texte des Addons (deutsch, englisch); Schlüssel beginnen mit der Kennung
    pub texte: (texte::Texte, texte::Texte),
}

/// Alle Addons in der Reihenfolge von Startseite und Hilfe.
pub const ALLE: [&Addon; 3] = [&ideen::ADDON, &quiz::ADDON, &zeichenbrett::ADDON];

/// Tabellen aller Addons für den Speicher.
pub fn daten() -> Vec<&'static Zusatz> { ALLE.iter().filter_map(|a| a.daten).collect() }

pub fn nach_art(art: &str) -> Option<&'static Addon> { ALLE.into_iter().find(|a| a.art == art) }
pub fn nach_kennung(kennung: &str) -> Option<&'static Addon> { ALLE.into_iter().find(|a| a.kennung == kennung) }

/// `NEXTPAD_ADDONS`: nicht gesetzt = alle, leer = keins, sonst Kennungen mit Komma; Unbekanntes ist ein Fehler.
pub fn auswahl(wert: Option<&str>) -> anyhow::Result<Vec<&'static str>> {
    let Some(w) = wert else { return Ok(ALLE.iter().map(|a| a.kennung).collect()) };
    let mut out = Vec::new();
    for k in w.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        let a = nach_kennung(k).ok_or_else(|| anyhow::anyhow!("NEXTPAD_ADDONS: unbekanntes Addon „{k}“, bekannt sind {}", ALLE.iter().map(|a| a.kennung).collect::<Vec<_>>().join(", ")))?;
        if !out.contains(&a.kennung) { out.push(a.kennung); }
    }
    Ok(out)
}

pub fn aktiv(state: &AppState, a: &Addon) -> bool { state.config.addons.contains(&a.kennung) }
pub fn aktive(state: &AppState) -> impl Iterator<Item = &'static Addon> + '_ { ALLE.into_iter().filter(|a| aktiv(state, a)) }

/// Addon eines Pads; None beim Text-Pad.
pub fn von_pad(state: &AppState, pad_id: &str) -> Option<&'static Addon> { state.store.kind(pad_id).as_deref().and_then(nach_art) }

/// Routen der eingeschalteten Addons.
pub fn routen(state: &AppState) -> Router<Arc<AppState>> { aktive(state).fold(Router::new(), |r, a| r.merge((a.routen)())) }

/// Antwort für Pads eines ausgeschalteten Addons; die Daten bleiben.
pub fn nicht_aktiv(a: &Addon, headers: &HeaderMap) -> Response {
    let sp = Sprache::aus_anfrage(headers);
    crate::web::error_page(sp, StatusCode::NOT_FOUND, &texte::tf(sp, "addon.nicht_aktiv", &[("name", texte::t(sp, a.name))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auswahl_der_addons() {
        assert_eq!(auswahl(None).unwrap(), vec!["ideen", "quiz", "zeichenbrett"]);
        assert!(auswahl(Some("")).unwrap().is_empty());
        assert_eq!(auswahl(Some(" quiz , quiz,ideen")).unwrap(), vec!["quiz", "ideen"]);
        assert!(auswahl(Some("quiz,umfrage")).is_err());
    }

    #[test]
    fn kennung_und_art_sind_eindeutig() {
        for (i, a) in ALLE.iter().enumerate() {
            for b in &ALLE[i + 1..] { assert!(a.kennung != b.kennung && a.art != b.art); }
            assert!([texte::Sprache::De, texte::Sprache::En].iter().all(|sp| texte::t(*sp, a.kachel).contains(&format!("/new?typ={}", a.kennung))), "Kachel von {} führt nicht zum eigenen Typ", a.kennung);
            assert!([texte::Sprache::De, texte::Sprache::En].iter().all(|sp| texte::t(*sp, a.sichern).ends_with('.')), "Satz zum Sichern von {} fehlt", a.kennung);
        }
    }
}
