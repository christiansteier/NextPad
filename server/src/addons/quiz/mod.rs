//! Quiz-Raster: Raster aus Kategorien und Punktwerten, die Leitung steuert am Beamer, Teams sammeln Punkte.
//! Fragen und Antworten liegen nur in SQLite, nie im Yjs-Dokument. Eine Antwort verlässt den Server nur zur Leitung
//! oder nachdem die Leitung sie aufgedeckt hat. Wer nicht leitet, sieht Raster, offene Frage und Punktestand, nur lesend.
use super::{Addon, Seite};
use crate::{texte::{self, Sprache}, web, AppState};
use daten::{Buzz, QuizField};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::broadcast;

pub mod daten;
mod messung;

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
/// Längster Countdown je Frage in Sekunden.
const MAX_SEKUNDEN: i64 = 120;

pub const QUIZ_JS: &str = include_str!("static/quiz.js");
pub const QUIZ_CSS: &str = include_str!("static/quiz.css");

const TEXTE_DE: &[(&str, &str)] = &[
    ("quiz.sichern", "Fragen und Punktestand lassen sich im Quiz als Markdown oder Text sichern."),
    ("messung.s.kicker", "Quiz-Raster · Buzzer-Messung"),
    ("messung.s.titel", "Buzzer-Messung"),
    ("messung.s.handy", "Diese Seite misst, wie schnell das WLAN im Raum Signale zustellt. Es gibt keine Punkte und es wird kein Name gespeichert, nur Zeiten und die Art des Geräts (iOS, Android, andere)."),
    ("messung.s.mitmessen", "Mitmessen"),
    ("messung.s.offen_lassen", "Bitte die Seite offen und den Bildschirm an lassen."),
    ("messung.s.warten", "Warten auf das Signal"),
    ("messung.s.aktiv_html", "{zahl} Geräte messen gerade mit"),
    ("messung.s.signal", "Signal senden"),
    ("messung.s.leertaste", "oder Leertaste"),
    ("messung.s.ablauf", "Ablauf: alle öffnen den QR-Code und tippen „Mitmessen“. Einige Minuten warten, dann 10 bis 20 Signale im Abstand von ein paar Sekunden senden; alle drücken, sobald „Jetzt“ erscheint."),
    ("messung.s.hinweis", "Gemessen wird, wie stark der Uhrabgleich eines Handys von Mal zu Mal schwankt, nicht ob seine Uhr richtig geht: Sind Hin- und Rückweg im WLAN dauerhaft verschieden lang, bleibt ein gleichbleibender Fehler unsichtbar. Die Reaktionszeit zählt ab dem Eintreffen des Signals und enthält die kurze Abfrage, bis der Knopf erscheint."),
    ("messung.s.geraete", "Geräte"),
    ("messung.s.geraet", "Gerät"),
    ("messung.s.art", "Art"),
    ("messung.s.abgleiche", "Abgleiche"),
    ("messung.s.laufzeit", "Laufzeit (Median)"),
    ("messung.s.uhr", "Uhr (95 %)"),
    ("messung.s.druecke", "Drücke"),
    ("messung.s.zuletzt", "zuletzt"),
    ("messung.s.csv", "Rohdaten als CSV"),
    ("messung.s.loeschen", "Messdaten löschen"),
    ("messung.s.zum_quiz", "Zurück zum Quiz"),
    ("messung.s.braucht_js", "Die Messung braucht JavaScript."),
    ("messung.s.seitentitel", "NextPad · Buzzer-Messung"),
    ("messung.zu_viele_geraete", "Es messen schon {n} Geräte mit."),
    ("messung.voll", "Die Messung ist voll. Bitte die Messdaten sichern und löschen."),
    ("messung.max_runden", "Höchstens {n} Runden."),
    ("messung.u.wenig", "Noch zu wenige Daten: mindestens drei Geräte, die einige Minuten mitmessen."),
    ("messung.u.traegt", "Der Uhrabgleich trägt: In 95 % der Fälle schwankt er um höchstens {u} ms, unter der Grenze von {grenze} ms."),
    ("messung.u.reicht_nicht", "Der Uhrabgleich reicht nicht: In 5 % der Fälle schwankt er um mehr als {u} ms, über der Grenze von {grenze} ms."),
    ("messungjs.fehler", "Fehler {status}"),
    ("messungjs.misst", "Misst mit: {n} Abgleiche, Laufzeit zuletzt {laufzeit}. Bitte offen lassen."),
    ("messungjs.unterbrochen", "Unterbrochen ({grund}), versuche es gleich wieder …"),
    ("messungjs.jetzt", "Jetzt drücken!"),
    ("messungjs.warten", "Warten auf das Signal"),
    ("messungjs.empfang_fehlt", "Empfang nicht gemeldet: {grund}"),
    ("messungjs.gedrueckt_nach", "Gedrückt nach {zeit}."),
    ("messungjs.nicht_angekommen", "Nicht angekommen: {grund}"),
    ("messungjs.w_geraete", "Geräte, Abgleiche, Runden"),
    ("messungjs.w_laufzeit", "Laufzeit hin und zurück"),
    ("messungjs.w_laufzeit_wert", "Median {median}, 95 % unter {p95}, über 300 ms: {ueber} %"),
    ("messungjs.w_uhr", "Uhr nach Abgleich (Schwankung)"),
    ("messungjs.w_uhr_wert", "Median {median}, 95 % unter {p95}"),
    ("messungjs.w_signal", "Signal erreicht die Handys"),
    ("messungjs.w_signal_wert", "nach {median} (95 %: {p95}); zwischen erstem und letztem Handy je Runde {spanne}, höchstens {max}"),
    ("messungjs.w_netz", "Netz verschiebt den Druck"),
    ("messungjs.w_netz_wert", "um {median} (95 %: {p95}); Unterschied zwischen Handys je Runde {spanne}, höchstens {max}"),
    ("messungjs.w_reaktion", "Reaktionszeit (ab Eintreffen des Signals)"),
    ("messungjs.w_reaktion_wert", "Median {median}"),
    ("messungjs.w_erster", "Wer war zuerst?"),
    ("messungjs.w_erster_wert", "{runden} Runden mit mindestens zwei Drücken: Erster nach Eingang beim Server falsch in {eingang}, Erster nach Reaktionszeit anders in {reaktion}, knapper als 50 ms in {knapp}"),
    ("messungjs.w_erster_keine", "noch keine Runde mit zwei Drücken"),
    ("messungjs.geraet", "Gerät {n}"),
    ("messungjs.vor", "vor {n} s"),
    ("messungjs.andere", "andere"),
    ("messungjs.loeschen_frage", "Alle Messdaten dieses Quiz löschen? Vorher die CSV sichern, wenn sie gebraucht wird."),
    ("quiz.s.start", "Spiel starten"),
    ("quiz.s.neu", "Neues Spiel"),
    ("quiz.s.stop", "Spiel beenden"),
    ("quiz.s.mehr", "Weitere Funktionen"),
    ("quiz.s.undo", "Rückgängig"),
    ("quiz.s.qr", "QR-Code"),
    ("quiz.s.vollbild", "Vollbild"),
    ("quiz.s.bearbeiten", "Fragen bearbeiten"),
    ("quiz.s.messung", "Buzzer-Messung"),
    ("quiz.s.ton_an", "Buzzer-Ton an"),
    ("quiz.s.mitmessen", "An der Messung teilnehmen"),
    ("quiz.s.setup_h", "Fragen und Teams"),
    ("quiz.s.titel", "Titel"),
    ("quiz.s.teams", "Teams, durch Komma getrennt"),
    ("quiz.s.sek", "Zeit je Frage in Sekunden (0 = ohne Countdown)"),
    ("quiz.s.tabelle", "Fragen als Tabelle: Kategorie | Punkte | Frage | Antwort"),
    ("quiz.s.tabelle_hinweis", "Eine Zeile je Frage, als Markdown-Tabelle oder aus einem Tabellenprogramm kopiert. Bis {kategorien} Kategorien und {fragen} Fragen. Die Antworten sieht nur die Leitung, bis sie aufgedeckt werden."),
    ("quiz.s.speichern", "Speichern und neu beginnen"),
    ("quiz.s.null", "Punkte beginnen wieder bei null."),
    ("quiz.s.wach_html", "Der Bildschirm bleibt nicht von selbst an. Bitte die automatische Sperre kurz ausschalten (iPhone: Einstellungen › Anzeige &amp; Helligkeit › Automatische Sperre; Android: Einstellungen › Display › Bildschirm-Timeout)."),
    ("quiz.s.raster", "Raster"),
    ("quiz.s.punktestand", "Punktestand"),
    ("quiz.s.braucht_js", "Das Quiz-Raster braucht JavaScript."),
    ("quiz.s.sichern", "Punktestand sichern"),
    ("quiz.s.seitentitel", "NextPad · Quiz-Raster"),
    ("quiz.s.mitmachen_moodle", "Mitmachen über Moodle:"),
    ("quiz.s.mitsehen", "Mitsehen auf dem Handy:"),
    ("quiz.kein_quiz", "Das ist kein Quiz-Raster."),
    ("quiz.nur_leitung", "Nur die Leitung des Quiz darf das."),
    ("quiz.team_fehlt", "Bitte mindestens ein Team eintragen."),
    ("quiz.max_teams", "Höchstens {n} Teams."),
    ("quiz.zeit", "Zeit je Frage: 0 bis {n} Sekunden."),
    ("quiz.laeuft_nicht", "Das Spiel läuft nicht. Erst „Spiel starten“."),
    ("quiz.gespielt", "Diese Frage ist schon gespielt. „Rückgängig“ holt sie zurück."),
    ("quiz.nur_offene", "Gewertet wird nur die offene Frage."),
    ("quiz.kein_team", "Dieses Team gibt es nicht."),
    ("quiz.buzzer_zu", "Der Buzzer ist gerade zu."),
    ("quiz.team_fest", "Im laufenden Spiel lässt sich das Team nicht wechseln."),
    ("quiz.t.spalten", "Zeile hat weniger als vier Spalten: „{zeile}“"),
    ("quiz.t.punkte", "Punkte sind keine Zahl: „{zeile}“"),
    ("quiz.t.fehlt", "Kategorie oder Frage fehlt: „{zeile}“"),
    ("quiz.t.max_fragen", "Höchstens {n} Fragen."),
    ("quiz.t.keine", "Keine Fragen gefunden."),
    ("quiz.t.max_kategorien", "Höchstens {n} Kategorien."),
    ("quizjs.zum_kurs", "Zurück zum Kurs"),
    ("quizjs.gedrueckt", "Gedrückt."),
    ("quizjs.schon", "Schon gedrückt."),
    ("quizjs.nicht_angekommen", "Nicht angekommen: {grund}"),
    ("quizjs.buzzer_fuer", "Buzzer für"),
    ("quizjs.team_waehlen", "Für den Buzzer dein Team wählen:"),
    ("quizjs.team_wechseln", "Team wechseln"),
    ("quizjs.bereit_gleich", "Bereit. Das Spiel beginnt gleich."),
    ("quizjs.bereit_frage", "Bereit. Der Buzzer erscheint, sobald die Leitung eine Frage öffnet."),
    ("quizjs.ton_an", "Buzzer-Ton an"),
    ("quizjs.ton_aus", "Buzzer-Ton aus"),
    ("quizjs.zeit_um", "Zeit um"),
    ("quizjs.zeit_ist_um", "Die Zeit ist um."),
    ("quizjs.buzzer", "Buzzer"),
    ("quizjs.verbinde", "Verbinde …"),
    ("quizjs.buzzer_team", "Buzzer für {team}"),
    ("quizjs.und", " und "),
    ("quizjs.gleichauf_namen", "{namen} gleichauf"),
    ("quizjs.jetzt_dran", "Jetzt dran"),
    ("quizjs.gleichzeitig", "Gleichzeitig gedrückt"),
    ("quizjs.zuerst", "Zuerst gedrückt"),
    ("quizjs.reihenfolge", "Buzzer: Reihenfolge wird ermittelt …"),
    ("quizjs.gleichauf", "gleichauf"),
    ("quizjs.zu_spaet", "zu spät"),
    ("quizjs.unsicher", "Zeit unsicher"),
    ("quizjs.falsch", "falsch geantwortet"),
    ("quizjs.fehler", "Fehler {status}"),
    ("quizjs.feld", "{kategorie}, {punkte} Punkte"),
    ("quizjs.feld_gespielt", "{kategorie}, {punkte} Punkte, gespielt"),
    ("quizjs.meta", "{kategorie} · {punkte} Punkte"),
    ("quizjs.aufdecken", "Antwort aufdecken"),
    ("quizjs.dran", "Dran: {team}"),
    ("quizjs.plus", "{team} plus {n}"),
    ("quizjs.minus", "{team} minus {n}"),
    ("quizjs.niemand", "Niemand, Feld schließen"),
    ("quizjs.zum_raster", "Zurück zum Raster"),
    ("quizjs.offen", "{offen} von {alle} Fragen offen"),
    ("quizjs.ergebnis", "Ergebnis"),
    ("quizjs.gleichstand", "Gleichstand: {namen}"),
    ("quizjs.gewinnt", "{team} gewinnt"),
    ("quizjs.nachbesprechen", "Zum Nachbesprechen"),
    ("quizjs.schwer", "Diese Fragen hat niemand oder erst nach einem Fehlversuch gelöst:"),
    ("quizjs.alle_geloest", "Alle Fragen wurden auf Anhieb gelöst."),
    ("quizjs.stat_team", "{team}: im Mittel {zeit} bis zum Buzzer ({n}× gedrückt)"),
    ("quizjs.schnellste", "Am schnellsten: {team} mit {zeit}"),
    ("quizjs.neu_hinweis", "„Neues Spiel“ setzt Punkte und Felder zurück, Fragen und Teams bleiben. „Rückgängig“ holt die letzte Wertung zurück und setzt das Spiel fort."),
    ("quizjs.name", "Quiz-Raster"),
    ("quizjs.leitung", "Quiz-Raster · Leitung"),
    ("quizjs.mitsehen", "Quiz-Raster · Mitsehen"),
    ("quizjs.vorbereitung", "Vorbereitung: Auf den Handys wählen jetzt alle ihr Team. „Spiel starten“ gibt das Raster frei, danach sind die Teams fest."),
    ("quizjs.richtet_ein", "Die Leitung richtet das Quiz gerade ein."),
    ("quizjs.neu_beginnen", "Neu beginnen? Alle Punkte gehen verloren."),
    ("quizjs.nicht_gespeichert", "Nicht gespeichert: {grund}"),
    ("quizjs.beenden_frage", "Spiel beenden und das Ergebnis zeigen? Nicht gespielte Felder zählen nicht."),
    ("quizjs.neues_frage", "Neues Spiel beginnen? Punkte und gespielte Felder werden zurückgesetzt, Fragen und Teams bleiben."),
    ("quiz.name", "Quiz-Raster"),
    ("quiz.kachel_html", r#"<li><a class="template" data-tint="blue" href="{base}/new?typ=quiz"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z"/></svg><span class="template-name">Quiz-Raster</span><span class="template-desc">Fragen nach Punkten, Teams, Beamer</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>"#),
    ("quiz.hilfe_html", r#"<h2>Quiz-Raster</h2><p>„Quiz-Raster“ legt ein Quiz nach Kategorien und Punktwerten an. Wer es anlegt, leitet: Fragen als Tabelle <code>Kategorie | Punkte | Frage | Antwort</code> einfügen, Teams eintragen, am Beamer ein Feld wählen, die Antwort aufdecken und Punkte vergeben. Wer den Link oder den QR-Code hat, sieht Raster, Frage und Punktestand mit, aber nicht die Antworten, bevor sie aufgedeckt sind. Tasten für die Leitung: Leertaste deckt auf, Esc führt zurück zum Raster, Z nimmt die letzte Wertung zurück. Gespielte Felder sind gesperrt; „Rückgängig“ holt die letzte Frage zurück. Rückgängig, QR-Code, Vollbild, Fragen bearbeiten, Buzzer-Messung und der Buzzer-Ton liegen im Menü ⋯ oben rechts.</p>
<p>Ablauf: Nach dem Speichern ist das Raster gesperrt, alle wählen auf dem Handy ihr Team und dürfen noch wechseln. „Spiel starten“ gibt das Raster frei, danach sind die Teams fest. „Spiel beenden“ zeigt jederzeit das Ergebnis, auch wenn noch Felder offen sind; nach der letzten Frage erscheint es von selbst. Das Ergebnis zeigt die Platzierung, die Buzzer-Statistik (schnellstes Team, mittlere Zeit bis zum Druck je Team) und die Fragen, die niemand oder erst nach einem Fehlversuch gelöst hat, zum Nachbesprechen. „Neues Spiel“ setzt Punkte und Felder zurück, Fragen und Teams bleiben.</p>
<p>Buzzer: Wer mitsieht, wählt auf dem Handy sein Team. Solange eine Frage offen ist und die Antwort nicht aufgedeckt, erscheint ein großer Buzzer. Gewertet wird, wann gedrückt wurde, nicht wann der Druck ankommt: Das Handy gleicht dazu laufend seine Uhr mit dem Server ab. Eine halbe Sekunde nach dem ersten Druck steht die Reihenfolge fest; liegen zwei Teams weniger als 50 ms auseinander, steht „gleichauf“ daneben. Je Handy und Frage zählt ein Druck. Am Beamer erscheint groß, welches Team zuerst gedrückt hat, und beim ersten Druck ertönt ein Buzzer-Ton (im Menü abschaltbar; der Browser spielt ihn erst, nachdem am Beamer einmal geklickt wurde). Die Punkte vergibt weiterhin die Leitung. Neben dem Team steht die Reaktionszeit ab dem Öffnen der Frage. Zieht die Leitung nach einer falschen Antwort Punkte ab, bleibt die Frage offen und am Beamer steht „Jetzt dran“ mit dem nächsten Team der Reihenfolge.</p>
<p>Countdown: Beim Anlegen lässt sich die Zeit je Frage einstellen (Vorgabe 10 Sekunden, 0 schaltet den Countdown ab). Die Zeit läuft groß am Beamer, ab dem Öffnen der Frage; danach nimmt der Buzzer keinen Druck mehr an. Wer vorher gedrückt hat, bleibt in der Reihenfolge.</p>
<p>„Buzzer-Messung“ (für die Leitung) prüft, ob ein Buzzer auf dem Handy im Raum fair wäre: Alle öffnen den QR-Code oder tippen im Quiz oben rechts auf „An der Messung teilnehmen“ (der Knopf erscheint, solange die Leitung die Messseite offen hat) und dann auf „Mitmessen“, die Leitung sendet einige Signale, alle drücken, sobald „Jetzt“ erscheint. Die Seite zeigt Laufzeiten, wie stark der Uhrabgleich der Handys schwankt (nicht, ob ihre Uhren richtig gehen), und ein Urteil. Gespeichert werden nur Zeiten und die Art des Geräts, keine Namen; die Leitung kann die Daten als CSV sichern und löschen.</p>"#),
    ("quiz.export_formate", "Quiz-Raster lassen sich als Markdown oder Text sichern (nur der Punktestand)."),
];
const TEXTE_EN: &[(&str, &str)] = &[
    ("quiz.sichern", "Questions and scores can be saved in the quiz as Markdown or text."),
    ("messung.s.kicker", "Quiz grid · Buzzer measurement"),
    ("messung.s.titel", "Buzzer measurement"),
    ("messung.s.handy", "This page measures how quickly the Wi-Fi in the room delivers signals. There are no points and no name is stored, only times and the type of device (iOS, Android, other)."),
    ("messung.s.mitmessen", "Measure"),
    ("messung.s.offen_lassen", "Please keep the page open and the screen on."),
    ("messung.s.warten", "Waiting for the signal"),
    ("messung.s.aktiv_html", "{zahl} devices are measuring"),
    ("messung.s.signal", "Send signal"),
    ("messung.s.leertaste", "or Space"),
    ("messung.s.ablauf", "How it works: everyone opens the QR code and taps “Measure”. Wait a few minutes, then send 10 to 20 signals a few seconds apart; everyone presses as soon as “Now” appears."),
    ("messung.s.hinweis", "What is measured is how much a phone’s clock sync varies from one time to the next, not whether its clock is right: if the way there and back in the Wi-Fi is persistently of different length, a constant error stays invisible. The reaction time counts from the arrival of the signal and includes the short query until the button appears."),
    ("messung.s.geraete", "Devices"),
    ("messung.s.geraet", "Device"),
    ("messung.s.art", "Type"),
    ("messung.s.abgleiche", "Syncs"),
    ("messung.s.laufzeit", "Round trip (median)"),
    ("messung.s.uhr", "Clock (95 %)"),
    ("messung.s.druecke", "Presses"),
    ("messung.s.zuletzt", "last"),
    ("messung.s.csv", "Raw data as CSV"),
    ("messung.s.loeschen", "Delete measurement data"),
    ("messung.s.zum_quiz", "Back to the quiz"),
    ("messung.s.braucht_js", "The measurement needs JavaScript."),
    ("messung.s.seitentitel", "NextPad · Buzzer measurement"),
    ("messung.zu_viele_geraete", "{n} devices are already measuring."),
    ("messung.voll", "The measurement is full. Please save and delete the measurement data."),
    ("messung.max_runden", "At most {n} rounds."),
    ("messung.u.wenig", "Not enough data yet: at least three devices measuring for a few minutes."),
    ("messung.u.traegt", "The clock sync holds: in 95 % of cases it varies by at most {u} ms, below the limit of {grenze} ms."),
    ("messung.u.reicht_nicht", "The clock sync is not good enough: in 5 % of cases it varies by more than {u} ms, above the limit of {grenze} ms."),
    ("messungjs.fehler", "Error {status}"),
    ("messungjs.misst", "Measuring: {n} syncs, last round trip {laufzeit}. Please keep open."),
    ("messungjs.unterbrochen", "Interrupted ({grund}), trying again shortly …"),
    ("messungjs.jetzt", "Now – press!"),
    ("messungjs.warten", "Waiting for the signal"),
    ("messungjs.empfang_fehlt", "Receipt not reported: {grund}"),
    ("messungjs.gedrueckt_nach", "Pressed after {zeit}."),
    ("messungjs.nicht_angekommen", "Did not arrive: {grund}"),
    ("messungjs.w_geraete", "Devices, syncs, rounds"),
    ("messungjs.w_laufzeit", "Round trip"),
    ("messungjs.w_laufzeit_wert", "Median {median}, 95 % below {p95}, above 300 ms: {ueber} %"),
    ("messungjs.w_uhr", "Clock after sync (variation)"),
    ("messungjs.w_uhr_wert", "Median {median}, 95 % below {p95}"),
    ("messungjs.w_signal", "Signal reaches the phones"),
    ("messungjs.w_signal_wert", "after {median} (95 %: {p95}); between first and last phone per round {spanne}, at most {max}"),
    ("messungjs.w_netz", "Network delays the press"),
    ("messungjs.w_netz_wert", "by {median} (95 %: {p95}); difference between phones per round {spanne}, at most {max}"),
    ("messungjs.w_reaktion", "Reaction time (from arrival of the signal)"),
    ("messungjs.w_reaktion_wert", "Median {median}"),
    ("messungjs.w_erster", "Who was first?"),
    ("messungjs.w_erster_wert", "{runden} rounds with at least two presses: first by arrival at the server wrong in {eingang}, first by reaction time different in {reaktion}, closer than 50 ms in {knapp}"),
    ("messungjs.w_erster_keine", "no round with two presses yet"),
    ("messungjs.geraet", "Device {n}"),
    ("messungjs.vor", "{n} s ago"),
    ("messungjs.andere", "other"),
    ("messungjs.loeschen_frage", "Delete all measurement data of this quiz? Save the CSV first if it is needed."),
    ("quiz.s.start", "Start game"),
    ("quiz.s.neu", "New game"),
    ("quiz.s.stop", "End game"),
    ("quiz.s.mehr", "More functions"),
    ("quiz.s.undo", "Undo"),
    ("quiz.s.qr", "QR code"),
    ("quiz.s.vollbild", "Full screen"),
    ("quiz.s.bearbeiten", "Edit questions"),
    ("quiz.s.messung", "Buzzer measurement"),
    ("quiz.s.ton_an", "Buzzer sound on"),
    ("quiz.s.mitmessen", "Take part in the measurement"),
    ("quiz.s.setup_h", "Questions and teams"),
    ("quiz.s.titel", "Title"),
    ("quiz.s.teams", "Teams, separated by commas"),
    ("quiz.s.sek", "Time per question in seconds (0 = no countdown)"),
    ("quiz.s.tabelle", "Questions as a table: Category | Points | Question | Answer"),
    ("quiz.s.tabelle_hinweis", "One line per question, as a Markdown table or copied from a spreadsheet. Up to {kategorien} categories and {fragen} questions. Only the lead sees the answers until they are revealed."),
    ("quiz.s.speichern", "Save and start over"),
    ("quiz.s.null", "Points start again from zero."),
    ("quiz.s.wach_html", "The screen does not stay on by itself. Please switch off auto-lock for a moment (iPhone: Settings › Display &amp; Brightness › Auto-Lock; Android: Settings › Display › Screen timeout)."),
    ("quiz.s.raster", "Grid"),
    ("quiz.s.punktestand", "Scores"),
    ("quiz.s.braucht_js", "The quiz grid needs JavaScript."),
    ("quiz.s.sichern", "Save scores"),
    ("quiz.s.seitentitel", "NextPad · Quiz grid"),
    ("quiz.s.mitmachen_moodle", "Take part through Moodle:"),
    ("quiz.s.mitsehen", "Follow on your phone:"),
    ("quiz.kein_quiz", "This is not a quiz grid."),
    ("quiz.nur_leitung", "Only the quiz lead can do this."),
    ("quiz.team_fehlt", "Please enter at least one team."),
    ("quiz.max_teams", "At most {n} teams."),
    ("quiz.zeit", "Time per question: 0 to {n} seconds."),
    ("quiz.laeuft_nicht", "The game is not running. Choose “Start game” first."),
    ("quiz.gespielt", "This question has already been played. “Undo” brings it back."),
    ("quiz.nur_offene", "Only the open question can be scored."),
    ("quiz.kein_team", "This team does not exist."),
    ("quiz.buzzer_zu", "The buzzer is closed at the moment."),
    ("quiz.team_fest", "The team cannot be changed during the game."),
    ("quiz.t.spalten", "Line has fewer than four columns: “{zeile}”"),
    ("quiz.t.punkte", "Points are not a number: “{zeile}”"),
    ("quiz.t.fehlt", "Category or question missing: “{zeile}”"),
    ("quiz.t.max_fragen", "At most {n} questions."),
    ("quiz.t.keine", "No questions found."),
    ("quiz.t.max_kategorien", "At most {n} categories."),
    ("quizjs.zum_kurs", "Back to course"),
    ("quizjs.gedrueckt", "Pressed."),
    ("quizjs.schon", "Already pressed."),
    ("quizjs.nicht_angekommen", "Did not arrive: {grund}"),
    ("quizjs.buzzer_fuer", "Buzzer for"),
    ("quizjs.team_waehlen", "Choose your team for the buzzer:"),
    ("quizjs.team_wechseln", "Change team"),
    ("quizjs.bereit_gleich", "Ready. The game is about to start."),
    ("quizjs.bereit_frage", "Ready. The buzzer appears as soon as the lead opens a question."),
    ("quizjs.ton_an", "Buzzer sound on"),
    ("quizjs.ton_aus", "Buzzer sound off"),
    ("quizjs.zeit_um", "Time’s up"),
    ("quizjs.zeit_ist_um", "Time is up."),
    ("quizjs.buzzer", "Buzzer"),
    ("quizjs.verbinde", "Connecting …"),
    ("quizjs.buzzer_team", "Buzzer for {team}"),
    ("quizjs.und", " and "),
    ("quizjs.gleichauf_namen", "{namen} tied"),
    ("quizjs.jetzt_dran", "Your turn"),
    ("quizjs.gleichzeitig", "Pressed at the same time"),
    ("quizjs.zuerst", "Pressed first"),
    ("quizjs.reihenfolge", "Buzzer: working out the order …"),
    ("quizjs.gleichauf", "tied"),
    ("quizjs.zu_spaet", "too late"),
    ("quizjs.unsicher", "time uncertain"),
    ("quizjs.falsch", "wrong answer"),
    ("quizjs.fehler", "Error {status}"),
    ("quizjs.feld", "{kategorie}, {punkte} points"),
    ("quizjs.feld_gespielt", "{kategorie}, {punkte} points, played"),
    ("quizjs.meta", "{kategorie} · {punkte} points"),
    ("quizjs.aufdecken", "Reveal answer"),
    ("quizjs.dran", "Turn: {team}"),
    ("quizjs.plus", "{team} plus {n}"),
    ("quizjs.minus", "{team} minus {n}"),
    ("quizjs.niemand", "Nobody, close field"),
    ("quizjs.zum_raster", "Back to the grid"),
    ("quizjs.offen", "{offen} of {alle} questions open"),
    ("quizjs.ergebnis", "Result"),
    ("quizjs.gleichstand", "Tie: {namen}"),
    ("quizjs.gewinnt", "{team} wins"),
    ("quizjs.nachbesprechen", "To discuss"),
    ("quizjs.schwer", "Nobody solved these questions, or only after a wrong attempt:"),
    ("quizjs.alle_geloest", "All questions were solved at the first attempt."),
    ("quizjs.stat_team", "{team}: on average {zeit} to the buzzer (pressed {n}×)"),
    ("quizjs.schnellste", "Fastest: {team} with {zeit}"),
    ("quizjs.neu_hinweis", "“New game” resets points and fields; questions and teams stay. “Undo” brings back the last scoring and continues the game."),
    ("quizjs.name", "Quiz grid"),
    ("quizjs.leitung", "Quiz grid · Lead"),
    ("quizjs.mitsehen", "Quiz grid · Following"),
    ("quizjs.vorbereitung", "Preparation: everyone now chooses their team on their phone. “Start game” releases the grid; after that, the teams are fixed."),
    ("quizjs.richtet_ein", "The lead is setting up the quiz."),
    ("quizjs.neu_beginnen", "Start over? All points will be lost."),
    ("quizjs.nicht_gespeichert", "Not saved: {grund}"),
    ("quizjs.beenden_frage", "End the game and show the result? Fields not played do not count."),
    ("quizjs.neues_frage", "Start a new game? Points and played fields are reset; questions and teams stay."),
    ("quiz.name", "Quiz grid"),
    ("quiz.kachel_html", r#"<li><a class="template" data-tint="blue" href="{base}/new?typ=quiz"><svg class="template-icon" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z"/></svg><span class="template-name">Quiz grid</span><span class="template-desc">Questions by points, teams, projector</span><svg class="chev" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" focusable="false"><path d="M9 6l6 6-6 6"/></svg></a></li>"#),
    ("quiz.hilfe_html", r#"<h2>Quiz grid</h2><p>“Quiz grid” creates a quiz by categories and point values. The person who creates it leads: paste the questions as a table <code>Category | Points | Question | Answer</code>, enter the teams, choose a field at the projector, reveal the answer and award points. Anyone with the link or the QR code follows the grid, the question and the scores, but not the answers before they are revealed. Keys for the lead: Space reveals, Esc goes back to the grid, Z undoes the last scoring. Played fields are locked; “Undo” brings back the last question. Undo, QR code, full screen, editing questions, buzzer measurement and the buzzer sound are in the ⋯ menu at the top right.</p>
<p>Flow: after saving, the grid is locked; everyone chooses their team on their phone and may still switch. “Start game” releases the grid, after that the teams are fixed. “End game” shows the result at any time, even with fields still open; after the last question it appears by itself. The result shows the ranking, the buzzer statistics (fastest team, average time to press per team) and the questions that nobody solved or that were only solved after a wrong attempt, for discussion. “New game” resets points and fields; questions and teams stay.</p>
<p>Buzzer: anyone following chooses their team on their phone. While a question is open and the answer is not revealed, a large buzzer appears. What counts is when the buzzer was pressed, not when the press arrives: the phone keeps aligning its clock with the server. Half a second after the first press, the order is fixed; if two teams are less than 50 ms apart, “tied” appears next to them. One press counts per phone and question. The projector shows large which team pressed first, and a buzzer sound plays at the first press (can be switched off in the menu; the browser only plays it after one click at the projector). The lead still awards the points. Next to each team is the reaction time from opening the question. If the lead deducts points after a wrong answer, the question stays open and the projector shows “Your turn” with the next team in the order.</p>
<p>Countdown: when creating the quiz, the time per question can be set (default 10 seconds, 0 switches the countdown off). The time runs large at the projector from opening the question; after that the buzzer no longer accepts presses. Anyone who pressed before stays in the order.</p>
<p>“Buzzer measurement” (for the lead) checks whether a phone buzzer would be fair in the room: everyone opens the QR code or taps “Take part in the measurement” at the top right of the quiz (the button appears while the lead has the measurement page open) and then “Measure”; the lead sends a few signals and everyone presses as soon as “Now” appears. The page shows round-trip times, how much the phones’ clock alignment varies (not whether their clocks are right), and a verdict. Only times and the type of device are stored, no names; the lead can save the data as CSV and delete it.</p>"#),
    ("quiz.export_formate", "Quiz grids can be saved as Markdown or text (scores only)."),
];

pub const ADDON: Addon = Addon {
    kennung: "quiz",
    art: "quiz",
    name: "quiz.name",
    daten: Some(&daten::ZUSATZ),
    routen,
    seite,
    export: export_antwort,
    kachel: "quiz.kachel_html",
    sichern: "quiz.sichern",
    hilfe: "quiz.hilfe_html",
    moodle: Some(|store, pad, titel| store.quiz_set_title(pad, titel)),
    texte: (TEXTE_DE, TEXTE_EN),
};

fn routen() -> Router<Arc<AppState>> {
    Router::new()
        .route("/p/{id}/quiz", get(get_))
        .route("/p/{id}/quiz/live", get(live))
        .route("/p/{id}/quiz/fragen", post(setup))
        .route("/p/{id}/quiz/feld", post(open))
        .route("/p/{id}/quiz/wertung", post(score))
        .route("/p/{id}/quiz/rueckgaengig", post(undo))
        .route("/p/{id}/quiz/spiel", post(spiel))
        .route("/p/{id}/quiz/buzzer", post(buzzer))
        .route("/static/quiz.js", get(static_js))
        .route("/static/quiz.css", get(static_css))
        .merge(messung::routen())
}

fn seite(s: Seite) -> Response {
    let join = join_svg(s.state, s.headers, s.free, s.url_id, s.pad_id, "");
    let sp = Sprache::aus_anfrage(s.headers);
    let mut cfg = s.cfg;
    cfg["lang"] = json!(sp.kuerzel());
    cfg["texte"] = texte::texte_mit(sp, "quizjs.");
    page(s.state, sp, s.url_id, cfg, &join)
}

fn export_antwort(state: &AppState, pad: &str, format: &str, sp: Sprache) -> Response {
    export(state, pad, format, sp).unwrap_or_else(|| web::error_page(sp, StatusCode::NOT_FOUND, texte::t(sp, "quiz.export_formate")))
}

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
    let access = web::check_access(state, headers, id).map_err(|(s, m)| web::abgelehnt(headers, s, m))?;
    if state.store.kind(&access.pad.id).as_deref() != Some(ADDON.art) { return Err(web::abgelehnt(headers, StatusCode::NOT_FOUND, "quiz.kein_quiz")); }
    let author = access.author.map(|a| a.0);
    let creator = access.free && !access.readonly && author.is_some() && state.store.owner_of(&access.pad.id).ok().flatten() == author;
    let moderator = !access.readonly && author.as_deref().is_some_and(|a| state.store.is_moderator(&access.pad.id, a));
    Ok(Viewer { pad: access.pad.id, lead: creator || moderator })
}

/// Schreibende Aufrufe: nur die Leitung, nur von der eigenen Seite (Origin).
pub(crate) fn lead(state: &AppState, headers: &HeaderMap, id: &str) -> Result<Viewer, Response> {
    if !web::origin_ok(state, headers) || headers.get(header::ORIGIN).is_none() { return Err(StatusCode::FORBIDDEN.into_response()); }
    let v = viewer(state, headers, id)?;
    if !v.lead { return Err(web::abgelehnt(headers, StatusCode::FORBIDDEN, "quiz.nur_leitung")); }
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
pub fn parse_table(text: &str, sp: Sprache) -> Result<Vec<QuizField>, String> {
    let zeile = |k, l: &str| texte::tf(sp, k, &[("zeile", &clean(l, 80))]);
    let mehr = |k, n: usize| texte::tf(sp, k, &[("n", &n.to_string())]);
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() { continue; }
        let cells: Vec<String> = if line.contains('\t') { line.split('\t').map(|c| c.trim().to_string()).collect() }
            else { line.trim_start_matches('|').trim_end_matches('|').split('|').map(|c| c.trim().to_string()).collect() };
        if cells.iter().all(|c| c.is_empty() || c.trim_matches(':').chars().all(|ch| ch == '-')) { continue; }
        if cells[0].eq_ignore_ascii_case("kategorie") || cells[0].eq_ignore_ascii_case("category") { continue; }
        if cells.len() < 4 { return Err(zeile("quiz.t.spalten", line)); }
        let digits: String = cells[1].chars().filter(|c| c.is_ascii_digit()).collect();
        let points: i64 = digits.parse().ok().filter(|p| *p > 0 && *p <= 100_000).ok_or_else(|| zeile("quiz.t.punkte", line))?;
        let (cat, question, answer) = (clean(&cells[0], MAX_CAT), clean(&cells[2], MAX_QUESTION), clean(&cells[3..].join(" | "), MAX_ANSWER));
        if cat.is_empty() || question.is_empty() { return Err(zeile("quiz.t.fehlt", line)); }
        out.push(QuizField { id: out.len() as i64 + 1, cat, points, question, answer });
        if out.len() > MAX_FIELDS { return Err(mehr("quiz.t.max_fragen", MAX_FIELDS)); }
    }
    if out.is_empty() { return Err(texte::t(sp, "quiz.t.keine").into()); }
    if out.iter().map(|f| f.cat.as_str()).collect::<HashSet<_>>().len() > MAX_CATS { return Err(mehr("quiz.t.max_kategorien", MAX_CATS)); }
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
    let mut best: Vec<(i64, f64, bool, Option<f64>)> = Vec::new();
    for b in buzzes.iter().filter(|b| b.ts_ein <= window) {
        match best.iter_mut().find(|x| x.0 == b.team) {
            Some(x) if b.t_press < x.1 => { x.1 = b.t_press; x.2 = b.flagged; x.3 = b.reaktion; }
            Some(_) => {}
            None => best.push((b.team, b.t_press, b.flagged, b.reaktion)),
        }
    }
    best.sort_by(|a, b| a.1.total_cmp(&b.1));
    let t0 = best.first().map(|x| x.1).unwrap_or(0.0);
    let mut out = Vec::new();
    let mut prev = t0;
    for (i, (team, t, flagged, reaktion)) in best.iter().enumerate() {
        // Reaktionszeit nur, wenn die Druckzeit sicher ist
        let reaktion = reaktion.filter(|_| !flagged).map(|r| r.round());
        out.push(json!({ "team": team, "name": name(*team), "abstand": ((t - t0) * 10.0).round() / 10.0, "gleichauf": i > 0 && t - prev < BUZZ_GLEICH_MS, "unsicher": flagged, "spaet": false, "reaktion": reaktion }));
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

/// Statistik fürs Ergebnis: schnellste Reaktion des Spiels und je Team der Mittelwert der Reaktionen (ms).
pub fn statistik(reaktionen: &[(i64, f64)], teams: &[(i64, String)]) -> serde_json::Value {
    let name = |t: i64| teams.iter().find(|(id, _)| *id == t).map(|(_, n)| n.clone()).unwrap_or_default();
    let Some(&(st, sms)) = reaktionen.iter().filter(|(t, _)| teams.iter().any(|(id, _)| id == t)).min_by(|a, b| a.1.total_cmp(&b.1)) else { return serde_json::Value::Null };
    let mut je: Vec<_> = teams.iter().filter_map(|(id, n)| {
        let r: Vec<f64> = reaktionen.iter().filter(|(t, _)| t == id).map(|(_, r)| *r).collect();
        (!r.is_empty()).then(|| (n.clone(), r.iter().sum::<f64>() / r.len() as f64, r.len()))
    }).collect();
    je.sort_by(|a, b| a.1.total_cmp(&b.1));
    json!({ "schnellste": { "name": name(st), "ms": sms.round() },
            "teams": je.iter().map(|(n, m, k)| json!({ "name": n, "mittel": m.round(), "anzahl": k })).collect::<Vec<_>>() })
}

/// Druckzeit in Serverzeit aus Gerätezeit und Uhrabgleich; unplausibel (nach dem Eingang, vor dem Öffnen oder mehr
/// als Laufzeit plus 1,5 s vor dem Eingang) wird durch Eingang minus halbe Laufzeit ersetzt und markiert.
pub fn buzz_time(t: f64, off: f64, rtt: f64, ein: f64, open: f64) -> (f64, bool) {
    let p = t + off;
    let ok = (0.0..=3000.0).contains(&rtt) && p <= ein + 20.0 && p >= open - 20.0 && ein - p <= rtt + 1500.0;
    if ok { (p, false) } else { (ein - rtt.clamp(0.0, 3000.0) / 2.0, true) }
}

/// Stand für diese Person. Antworten nur für die Leitung oder nach dem Aufdecken.
pub async fn get_(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
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
    let open = state.store.quiz_buzz_open(&v.pad).ok().flatten().filter(|(f, _)| st.current == Some(*f));
    let mut buzz = match open {
        Some((c, _)) => state.store.quiz_buzzes(&v.pad, c).map(|b| buzz_order(&b, &teams, jetzt())).unwrap_or_default(),
        None => serde_json::Value::Null,
    };
    // Teams, die zu dieser Frage schon falsch geantwortet haben: dran ist das nächste in der Reihenfolge
    if let (Some(c), Some(reihe)) = (st.current, buzz.get_mut("reihe").and_then(|r| r.as_array_mut())) {
        for r in reihe.iter_mut() {
            let falsch = r["team"].as_i64().is_some_and(|t| log.iter().any(|(f, lt, d)| *f == c && *lt == Some(t) && *d < 0));
            r["falsch"] = json!(falsch);
        }
    }
    let current = current.map(|mut q| {
        q["sekunden"] = json!(st.sekunden);
        // Restzeit bis zum Ende des Countdowns; null ohne Countdown oder nach dem Aufdecken
        if let Some((_, ts)) = open.filter(|_| st.sekunden > 0 && !st.revealed) {
            q["restMs"] = json!((ts + st.sekunden as f64 * 1000.0 - jetzt()).max(0.0).round());
        }
        q
    });
    let statistik = statistik(&state.store.quiz_reaktionen(&v.pad).unwrap_or_default(), &teams);
    let phase = match st.phase { 0 => "vorbereitung", 2 => "ende", _ => "laeuft" };
    let mut out = json!({ "title": st.title, "lead": v.lead, "phase": phase, "teams": teams_json, "board": board, "current": current, "buzz": buzz, "statistik": statistik,
        "messung": !v.lead && messung::leitung_misst(&v.pad) });
    if v.lead {
        out["canUndo"] = json!(!log.is_empty());
        out["setup"] = json!({ "sekunden": st.sekunden, "title": st.title, "teams": teams.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "), "table": if fields.is_empty() { String::new() } else { table_text(&fields) } });
    }
    no_store(Json(out).into_response())
}

/// Fragen und Teams setzen; Punkte und offene Frage beginnen neu.
pub async fn setup(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let s = |k: &str| body.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let sp = Sprache::aus_anfrage(&headers);
    let fields = match parse_table(&s("table"), sp) { Ok(f) => f, Err(m) => return (StatusCode::BAD_REQUEST, m).into_response() };
    let teams: Vec<String> = s("teams").split(',').map(|t| clean(t, MAX_TEAM)).filter(|t| !t.is_empty()).collect();
    if teams.is_empty() { return web::abgelehnt(&headers, StatusCode::BAD_REQUEST, "quiz.team_fehlt"); }
    if teams.len() > MAX_TEAMS { return (StatusCode::BAD_REQUEST, texte::tf(sp, "quiz.max_teams", &[("n", &MAX_TEAMS.to_string())])).into_response(); }
    let title = clean(&s("title"), MAX_TITLE);
    let sekunden = body.get("sekunden").and_then(|x| x.as_i64()).unwrap_or(10);
    if !(0..=MAX_SEKUNDEN).contains(&sekunden) { return (StatusCode::BAD_REQUEST, texte::tf(sp, "quiz.zeit", &[("n", &MAX_SEKUNDEN.to_string())])).into_response(); }
    match state.store.quiz_replace(&v.pad, &title, &fields, &teams, sekunden) {
        Ok(()) => { state.live.notify(&v.pad); Json(json!({ "ok": true, "fields": fields.len() })).into_response() }
        Err(e) => { tracing::warn!("Quiz einrichten: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Frage öffnen, Antwort aufdecken oder zurück zum Raster (`field: null`).
pub async fn open(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let field = body.get("field").and_then(|x| x.as_i64());
    let revealed = body.get("revealed").and_then(|x| x.as_bool()).unwrap_or(false);
    // Fragen nur im laufenden Spiel; gespielte nicht versehentlich wieder öffnen, zurück holt sie nur „Rückgängig“
    if let Some(f) = field {
        match state.store.quiz_state(&v.pad) {
            Ok(st) if st.phase != 1 => return web::abgelehnt(&headers, StatusCode::CONFLICT, "quiz.laeuft_nicht"),
            Ok(_) => {}
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
        match state.store.quiz_log(&v.pad) {
            Ok(log) if played(&log).contains(&f) => return web::abgelehnt(&headers, StatusCode::CONFLICT, "quiz.gespielt"),
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
            state.live.notify(&v.pad);
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
    // gewertet wird nur die gerade offene Frage im laufenden Spiel
    match state.store.quiz_state(&v.pad) {
        Ok(st) if st.phase != 1 || st.current != Some(field) => return web::abgelehnt(&headers, StatusCode::CONFLICT, "quiz.nur_offene"),
        Ok(_) => {}
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    let team = body.get("team").and_then(|x| x.as_i64());
    let plus = body.get("plus").and_then(|x| x.as_bool()).unwrap_or(true);
    let points = match state.store.quiz_fields(&v.pad) { Ok(f) => f.iter().find(|f| f.id == field).map(|f| f.points), Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response() };
    let Some(points) = points else { return StatusCode::NOT_FOUND.into_response() };
    let delta = if team.is_none() { 0 } else if plus { points } else { -points };
    match state.store.quiz_score(&v.pad, field, team, delta) {
        Ok(true) => { state.live.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => { tracing::warn!("Quiz werten: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Spielablauf: `aktion` "start" (Raster frei, Teams fest), "ende" (Ergebnis zeigen, auch mit offenen Feldern)
/// oder "neu" (Punkte und Felder zurück, Fragen und Teams bleiben, wieder Vorbereitung).
pub async fn spiel(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    let r = match body.get("aktion").and_then(|x| x.as_str()) {
        Some("start") => state.store.quiz_set_phase(&v.pad, 1),
        Some("ende") => state.store.quiz_set_phase(&v.pad, 2),
        Some("neu") => state.store.quiz_new_game(&v.pad),
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    match r {
        Ok(()) => { state.live.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
        Err(e) => { tracing::warn!("Quiz Spielablauf: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

pub async fn undo(State(state): State<Arc<AppState>>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let v = match lead(&state, &headers, &id) { Ok(v) => v, Err(r) => return r };
    // nach „Spiel beenden“ macht Rückgängig auch das Beenden rückgängig: das Spiel läuft weiter
    if state.store.quiz_state(&v.pad).is_ok_and(|s| s.phase == 2) { let _ = state.store.quiz_set_phase(&v.pad, 1); }
    match state.store.quiz_undo(&v.pad) {
        Ok(_) => { state.live.notify(&v.pad); Json(json!({ "ok": true })).into_response() }
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
    if !teams.iter().any(|(tid, _)| *tid == team) { return web::abgelehnt(&headers, StatusCode::BAD_REQUEST, "quiz.kein_team"); }
    let Some((open_field, open_ts)) = open.filter(|(f, _)| st.current == Some(*f) && *f == field && !st.revealed)
        else { return web::abgelehnt(&headers, StatusCode::CONFLICT, "quiz.buzzer_zu") };
    // kein Teamwechsel im laufenden Spiel: ein Gerät buzzert immer für das Team, für das es zuerst gedrückt hat
    match state.store.quiz_buzz_team(&v.pad, dev) {
        Ok(Some(alt)) if alt != team => return web::abgelehnt(&headers, StatusCode::CONFLICT, "quiz.team_fest"),
        Ok(_) => {}
        Err(e) => { tracing::warn!("Buzzer: {e}"); return StatusCode::INTERNAL_SERVER_ERROR.into_response(); }
    }
    let (t_press, flagged) = buzz_time(t, off, rtt, ein, open_ts);
    // Countdown: gedrückt nach Ablauf zählt nicht; etwas Spielraum für den Weg durchs Netz
    if st.sekunden > 0 {
        let frist = open_ts + st.sekunden as f64 * 1000.0;
        if t_press > frist + 50.0 || ein > frist + 1500.0 { return (StatusCode::CONFLICT, "Die Zeit ist um.").into_response(); }
    }
    let reaktion = Some(t_press - open_ts).filter(|r| *r >= 0.0);
    match state.store.quiz_buzz_add(&v.pad, open_field, dev, &Buzz { team, t_press, ts_ein: ein, flagged, reaktion }, MAX_BUZZ) {
        Ok(added) => {
            if added { state.live.notify(&v.pad); }
            Json(json!({ "ok": true, "neu": added })).into_response()
        }
        Err(e) => { tracing::warn!("Buzzer: {e}"); StatusCode::INTERNAL_SERVER_ERROR.into_response() }
    }
}

/// Live-Meldungen: nur „geändert“, ohne Inhalt; den Stand holt jede Seite mit ihren eigenen Rechten.
/// Nutzt den Kanal der Ideensammlung, Pad-Kennungen sind eindeutig.
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

/// Export für alle mit dem Link: nur Titel und Punktestand, keine Fragen und keine Antworten.
pub fn export(state: &AppState, pad: &str, format: &str, sp: Sprache) -> Option<Response> {
    if !matches!(format, "md" | "txt") { return None; }
    let st = state.store.quiz_state(pad).ok()?;
    let teams = state.store.quiz_teams(pad).ok()?;
    let log = state.store.quiz_log(pad).ok()?;
    let mut rows: Vec<(String, i64)> = teams.iter().map(|(id, n)| (n.clone(), log.iter().filter(|(_, t, _)| *t == Some(*id)).map(|(_, _, d)| d).sum())).collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    let mut out = format!("# {}\n\n{}\n\n", if st.title.is_empty() { texte::t(sp, "quiz.name") } else { &st.title }, texte::t(sp, "quiz.s.punktestand"));
    for (n, p) in rows { out.push_str(&format!("- {n}: {p}\n")); }
    let ct = if format == "md" { "text/markdown; charset=utf-8" } else { "text/plain; charset=utf-8" };
    Some(([(header::CONTENT_TYPE, ct)], out).into_response())
}

/// QR-Code zum Mitsehen: nur bei freien Quiz über den Link selbst (nicht den Leselink, der Rechte hätte ohnehin keine).
/// `suffix`: Unterseite, auf die der Code führt (leer: das Quiz selbst).
pub fn join_svg(state: &AppState, headers: &HeaderMap, free: bool, url_id: &str, pad_id: &str, suffix: &str) -> String {
    // Moodle-Aktivität: der Code führt zur Aktivität in Moodle (Anmeldung dort), nicht direkt ins Pad
    if !free {
        return state.store.join_url(pad_id).map(|u| crate::qr::block(&u, texte::t(Sprache::aus_anfrage(headers), "quiz.s.mitmachen_moodle"))).unwrap_or_default();
    }
    if url_id.starts_with("r.") { return String::new(); }
    let base = &state.config.base_path;
    let origin = state.config.public_url.clone().unwrap_or_else(|| {
        let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
        format!("https://{host}{base}")
    });
    let url = format!("{}/p/{}{suffix}", origin.trim_end_matches('/'), web::urlencoding(pad_id));
    crate::qr::block(&url, texte::t(Sprache::aus_anfrage(headers), "quiz.s.mitsehen"))
}

/// Seite des Quiz; Leitung und Mitsehende bekommen dieselbe Seite, das Skript zeigt je nach Rolle.
pub fn page(state: &AppState, sp: Sprache, url_id: &str, cfg: serde_json::Value, join: &str) -> Response {
    let base = &state.config.base_path;
    let t = |k| texte::t(sp, k);
    let content = format!(r#"<main id="inhalt" class="quiz" tabindex="-1">
<div class="quiz-top"><div><p class="kicker" id="quiz-kicker">{name}</p><h1 id="quiz-title">{name}</h1></div>
<div class="quiz-tools" id="lead-tools" hidden>
<button type="button" class="btn btn-primary" id="q-start" hidden>{start}</button>
<button type="button" class="btn btn-primary" id="q-neu" hidden>{neu}</button>
<button type="button" class="btn btn-secondary" id="q-stop" hidden>{stop}</button>
<details class="quiz-menu" id="q-menu"><summary class="btn btn-secondary" title="{mehr}" aria-label="{mehr}">⋯</summary>
<div class="quiz-menu-list">
<button type="button" class="btn btn-secondary" id="q-undo">{undo}</button>
<button type="button" class="btn btn-secondary" id="q-qr" aria-controls="qr-box" aria-expanded="false">{qr}</button>
<button type="button" class="btn btn-secondary" id="q-full">{vollbild}</button>
<button type="button" class="btn btn-secondary" id="q-edit" aria-controls="setup" aria-expanded="false">{bearbeiten}</button>
<a class="btn btn-secondary" id="q-messung" href="{base}/p/{u}/messung">{messung}</a>
<button type="button" class="btn btn-secondary" id="q-ton" aria-pressed="true">{ton_an}</button>
</div></details>
</div>
<div class="quiz-tools" id="viewer-tools" hidden><a class="btn btn-primary" id="q-mitmessen" href="{base}/p/{u}/messung">{mitmessen}</a></div></div>
<section class="panel quiz-setup" id="setup" hidden aria-labelledby="setup-h">
<h2 id="setup-h">{setup_h}</h2>
<label class="field-label" for="in-title">{titel}</label><input id="in-title" maxlength="{MAX_TITLE}" autocomplete="off">
<label class="field-label" for="in-teams">{teams}</label><input id="in-teams" autocomplete="off" placeholder="Team 1, Team 2, Team 3">
<label class="field-label" for="in-sek">{sek}</label><input id="in-sek" type="number" min="0" max="{MAX_SEKUNDEN}" step="1" inputmode="numeric" value="10">
<label class="field-label" for="in-table">{tabelle}</label>
<textarea id="in-table" rows="12" spellcheck="false" aria-describedby="table-hint setup-error"></textarea>
<p class="hint" id="table-hint">{tabelle_hinweis}</p>
<p class="field-error" id="setup-error" role="alert" hidden></p>
<p><button type="button" class="btn btn-primary" id="q-save">{speichern}</button> <span class="hint">{null}</span></p>
</section>
<aside class="panel quiz-qr" id="qr-box" hidden>{join}</aside>
<section class="quiz-buzz-wahl" id="buzz-wahl" hidden aria-label="Buzzer"></section>
<p class="notice quiz-wach" id="buzz-wach" role="status" hidden>{wach}</p>
<p class="notice quiz-phase" id="q-phase" role="status" hidden></p>
<section class="quiz-board" id="board" aria-label="{raster}"></section>
<section class="quiz-question" id="question" hidden aria-live="polite">
<p class="quiz-meta" id="q-meta"></p><p class="quiz-countdown" id="q-countdown" hidden></p><p class="quiz-q" id="q-text"></p><p class="quiz-a" id="q-answer" hidden></p>
<p class="quiz-erster" id="buzz-erster" hidden aria-live="assertive"></p>
<button type="button" class="quiz-buzzer" id="buzz-btn" hidden>Buzzer</button>
<p class="hint" id="buzz-info" aria-live="polite"></p>
<p class="quiz-buzz-kopf" id="buzz-kopf" hidden></p><ol class="quiz-buzz-reihe" id="buzz-reihe" hidden aria-live="polite" aria-labelledby="buzz-kopf"></ol>
<div class="quiz-actions" id="q-actions"></div>
</section>
<section class="quiz-ende" id="ende" hidden aria-live="polite" aria-labelledby="ende-h"></section>
<p class="hint quiz-empty" id="empty" hidden></p>
<section class="quiz-scores" id="scores" aria-label="{punktestand}"></section>
<noscript><p class="notice">{braucht_js}</p></noscript>
<p class="hint quiz-export"><a href="{base}/p/{u}/export/md">{sichern}</a></p>
</main>
<script type="application/json" id="nextpad-config">{cfg}</script>"#,
        u = web::urlencoding(url_id), cfg = cfg.to_string().replace("</", "<\\/"),
        name = t("quiz.name"), start = t("quiz.s.start"), neu = t("quiz.s.neu"), stop = t("quiz.s.stop"), mehr = t("quiz.s.mehr"), undo = t("quiz.s.undo"),
        qr = t("quiz.s.qr"), vollbild = t("quiz.s.vollbild"), bearbeiten = t("quiz.s.bearbeiten"), messung = t("quiz.s.messung"), ton_an = t("quiz.s.ton_an"),
        mitmessen = t("quiz.s.mitmessen"), setup_h = t("quiz.s.setup_h"), titel = t("quiz.s.titel"), teams = t("quiz.s.teams"), sek = t("quiz.s.sek"),
        tabelle = t("quiz.s.tabelle"), speichern = t("quiz.s.speichern"), null = t("quiz.s.null"), wach = t("quiz.s.wach_html"), raster = t("quiz.s.raster"),
        punktestand = t("quiz.s.punktestand"), braucht_js = t("quiz.s.braucht_js"), sichern = t("quiz.s.sichern"),
        tabelle_hinweis = texte::tf(sp, "quiz.s.tabelle_hinweis", &[("kategorien", &MAX_CATS.to_string()), ("fragen", &MAX_FIELDS.to_string())]));
    let head = format!(r#"<link rel="stylesheet" href="{base}/static/quiz.css?v={v}">"#, v = version());
    let scripts = format!(r#"<script src="{base}/static/quiz.js?v={v}"></script>"#, v = version());
    let html = web::page_frame(state, sp, Some(&format!("/p/{}", web::urlencoding(url_id))), t("quiz.s.seitentitel"), &head, &content, &scripts);
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
        let f = parse_table("| Kategorie | Punkte | Frage | Antwort |\n| --- | --- | --- | --- |\n| Audio | 100 | Wie lang? | 5 Min. |\n| Audio | 200 | Was? | a | b |", Sprache::De).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].answer, "a | b");
        let t = parse_table("KI\t300\tWelcher Dienst?\tKI:connect.nrw", Sprache::De).unwrap();
        assert_eq!((t[0].cat.as_str(), t[0].points), ("KI", 300));
    }
    #[test]
    fn tabelle_fehler() {
        assert!(parse_table("", Sprache::De).is_err());
        assert!(parse_table("| a | b |", Sprache::De).is_err());
        assert!(parse_table("| a | x | Frage | Antwort |", Sprache::De).is_err());
    }
    #[test]
    fn gespielt_nur_nach_plus_oder_niemand() {
        let p = played(&[(1, Some(1), -100), (2, Some(2), 200), (3, None, 0)]);
        assert!(!p.contains(&1) && p.contains(&2) && p.contains(&3));
    }
    #[test]
    fn buzzer_reihenfolge_nach_druckzeit_im_fenster() {
        let teams = vec![(1, "Rot".to_string()), (2, "Blau".to_string()), (3, "Grün".to_string())];
        let b = |team, t_press, ts_ein| Buzz { team, t_press, ts_ein, flagged: false, reaktion: None };
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
