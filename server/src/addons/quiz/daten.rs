//! Tabellen und Zugriffe von Quiz-Raster, Buzzer und Buzzer-Messung.
use crate::store::{hat_spalte, Store, Zusatz};
use crate::ids;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

pub const ZUSATZ: Zusatz = Zusatz { schema: SCHEMA, ergaenzen, tabellen: &["quiz_buzz", "quiz_buzz_open", "quiz_log", "quiz_teams", "quiz_fields", "quiz_state", "messung_marke", "messung_runde", "messung_probe", "messung_geraet"] };

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS quiz_state (pad_id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', current INTEGER, revealed INTEGER NOT NULL DEFAULT 0, phase INTEGER NOT NULL DEFAULT 1, sekunden INTEGER NOT NULL DEFAULT 10);
CREATE TABLE IF NOT EXISTS quiz_fields (pad_id TEXT NOT NULL, id INTEGER NOT NULL, cat TEXT NOT NULL, points INTEGER NOT NULL, question TEXT NOT NULL, answer TEXT NOT NULL, PRIMARY KEY (pad_id, id));
CREATE TABLE IF NOT EXISTS quiz_teams (pad_id TEXT NOT NULL, id INTEGER NOT NULL, name TEXT NOT NULL, PRIMARY KEY (pad_id, id));
CREATE TABLE IF NOT EXISTS quiz_log (pad_id TEXT NOT NULL, seq INTEGER NOT NULL, field INTEGER NOT NULL, team INTEGER, delta INTEGER NOT NULL, PRIMARY KEY (pad_id, seq));
CREATE TABLE IF NOT EXISTS quiz_buzz (pad_id TEXT NOT NULL, field INTEGER NOT NULL, dev TEXT NOT NULL, team INTEGER NOT NULL, t_press REAL NOT NULL, ts_ein REAL NOT NULL, flagged INTEGER NOT NULL DEFAULT 0, reaktion REAL, PRIMARY KEY (pad_id, field, dev));
CREATE TABLE IF NOT EXISTS quiz_buzz_open (pad_id TEXT PRIMARY KEY, field INTEGER NOT NULL, ts REAL NOT NULL);
CREATE TABLE IF NOT EXISTS messung_geraet (pad_id TEXT NOT NULL, dev TEXT NOT NULL, nr INTEGER NOT NULL, art TEXT NOT NULL, PRIMARY KEY (pad_id, dev));
CREATE TABLE IF NOT EXISTS messung_probe (pad_id TEXT NOT NULL, nr INTEGER NOT NULL, ts REAL NOT NULL, rtt REAL NOT NULL, rtt_med REAL NOT NULL, off REAL NOT NULL);
CREATE INDEX IF NOT EXISTS messung_probe_pad ON messung_probe (pad_id);
CREATE TABLE IF NOT EXISTS messung_runde (pad_id TEXT NOT NULL, runde INTEGER NOT NULL, ts REAL NOT NULL, PRIMARY KEY (pad_id, runde));
CREATE TABLE IF NOT EXISTS messung_marke (pad_id TEXT NOT NULL, nr INTEGER NOT NULL, runde INTEGER NOT NULL, t_empfang REAL, t_druck REAL, ts_ein REAL, off REAL NOT NULL, rtt REAL NOT NULL, PRIMARY KEY (pad_id, nr, runde));
"#;

fn ergaenzen(conn: &Connection) -> rusqlite::Result<()> {
    // Schema-Ergänzung 28.10.2026: Spielphase im Quiz; laufende Spiele bleiben laufend (1)
    let has_phase: bool = hat_spalte(conn, "quiz_state", "phase")?;
    if !has_phase { conn.execute_batch("ALTER TABLE quiz_state ADD COLUMN phase INTEGER NOT NULL DEFAULT 1;")?; }
    // Schema-Ergänzung 28.10.2026: Countdown je Frage und Reaktionszeit je Buzzer-Druck
    let has_sek: bool = hat_spalte(conn, "quiz_state", "sekunden")?;
    if !has_sek { conn.execute_batch("ALTER TABLE quiz_state ADD COLUMN sekunden INTEGER NOT NULL DEFAULT 10;")?; }
    let has_reak: bool = hat_spalte(conn, "quiz_buzz", "reaktion")?;
    if !has_reak { conn.execute_batch("ALTER TABLE quiz_buzz ADD COLUMN reaktion REAL;")?; }
    Ok(())
}

#[derive(Debug, Clone, Default)]
/// `phase`: 0 Vorbereitung (Teams wählen, Raster gesperrt), 1 läuft, 2 beendet (Ergebnis).
/// `sekunden`: Countdown je Frage, 0 = ohne.
pub struct QuizState { pub title: String, pub current: Option<i64>, pub revealed: bool, pub phase: i64, pub sekunden: i64 }
#[derive(Debug, Clone, serde::Serialize)]
pub struct QuizField { pub id: i64, pub cat: String, pub points: i64, pub question: String, pub answer: String }

impl Store {
    // Quiz-Raster: Fragen und Antworten nur hier, nie im Yjs-Dokument
    pub fn quiz_state(&self, pad_id: &str) -> Result<QuizState> {
        Ok(self.c().query_row("SELECT title, current, revealed, phase, sekunden FROM quiz_state WHERE pad_id=?1", params![pad_id],
            |r| Ok(QuizState { title: r.get(0)?, current: r.get(1)?, revealed: r.get(2)?, phase: r.get(3)?, sekunden: r.get(4)? })).optional()?
            .unwrap_or(QuizState { sekunden: 10, ..Default::default() }))
    }
    pub fn quiz_fields(&self, pad_id: &str) -> Result<Vec<QuizField>> {
        Ok(self.c().prepare("SELECT id, cat, points, question, answer FROM quiz_fields WHERE pad_id=?1 ORDER BY id")?
            .query_map(params![pad_id], |r| Ok(QuizField { id: r.get(0)?, cat: r.get(1)?, points: r.get(2)?, question: r.get(3)?, answer: r.get(4)? }))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn quiz_teams(&self, pad_id: &str) -> Result<Vec<(i64, String)>> {
        Ok(self.c().prepare("SELECT id, name FROM quiz_teams WHERE pad_id=?1 ORDER BY id")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?)
    }
    /// Wertungen in Reihenfolge: (Feld, Team oder None für „niemand“, Punkte)
    pub fn quiz_log(&self, pad_id: &str) -> Result<Vec<(i64, Option<i64>, i64)>> {
        Ok(self.c().prepare("SELECT field, team, delta FROM quiz_log WHERE pad_id=?1 ORDER BY seq")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<std::result::Result<_, _>>()?)
    }
    /// Neue Fragen und Teams: Wertungen und offene Frage fallen weg.
    pub fn quiz_replace(&self, pad_id: &str, title: &str, fields: &[QuizField], teams: &[String], sekunden: i64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        for t in ["quiz_buzz", "quiz_buzz_open", "quiz_log", "quiz_teams", "quiz_fields"] { tx.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![pad_id])?; }
        for f in fields { tx.execute("INSERT INTO quiz_fields (pad_id, id, cat, points, question, answer) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![pad_id, f.id, f.cat, f.points, f.question, f.answer])?; }
        for (i, t) in teams.iter().enumerate() { tx.execute("INSERT INTO quiz_teams (pad_id, id, name) VALUES (?1, ?2, ?3)", params![pad_id, i as i64 + 1, t])?; }
        tx.execute("INSERT INTO quiz_state (pad_id, title, current, revealed, phase, sekunden) VALUES (?1, ?2, NULL, 0, 0, ?3) ON CONFLICT(pad_id) DO UPDATE SET title=?2, current=NULL, revealed=0, phase=0, sekunden=?3", params![pad_id, title, sekunden])?;
        tx.execute("UPDATE pads SET last_edited=?2 WHERE id=?1", params![pad_id, ids::now_ms()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn quiz_set_title(&self, pad_id: &str, title: &str) -> Result<()> {
        self.c().execute("INSERT INTO quiz_state (pad_id, title) VALUES (?1, ?2) ON CONFLICT(pad_id) DO UPDATE SET title=?2", params![pad_id, title])?;
        Ok(())
    }
    /// Offene Frage setzen (None: zurück zum Raster). false: Feld gibt es nicht.
    pub fn quiz_open(&self, pad_id: &str, field: Option<i64>, revealed: bool) -> Result<bool> {
        let c = self.c();
        if let Some(f) = field {
            if c.query_row("SELECT 1 FROM quiz_fields WHERE pad_id=?1 AND id=?2", params![pad_id, f], |_| Ok(())).optional()?.is_none() { return Ok(false); }
        }
        c.execute("INSERT INTO quiz_state (pad_id, current, revealed) VALUES (?1, ?2, ?3) ON CONFLICT(pad_id) DO UPDATE SET current=?2, revealed=?3", params![pad_id, field, field.is_some() && revealed])?;
        Ok(true)
    }
    /// Wertung eintragen. Plus oder „niemand“ schließt das Feld und führt zurück zum Raster. false: Feld oder Team gibt es nicht.
    pub fn quiz_score(&self, pad_id: &str, field: i64, team: Option<i64>, delta: i64) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let ok_field = tx.query_row("SELECT 1 FROM quiz_fields WHERE pad_id=?1 AND id=?2", params![pad_id, field], |_| Ok(())).optional()?.is_some();
        let ok_team = match team { Some(t) => tx.query_row("SELECT 1 FROM quiz_teams WHERE pad_id=?1 AND id=?2", params![pad_id, t], |_| Ok(())).optional()?.is_some(), None => true };
        if !ok_field || !ok_team { return Ok(false); }
        let seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM quiz_log WHERE pad_id=?1", params![pad_id], |r| r.get(0))?;
        tx.execute("INSERT INTO quiz_log (pad_id, seq, field, team, delta) VALUES (?1, ?2, ?3, ?4, ?5)", params![pad_id, seq, field, team, delta])?;
        if delta > 0 || team.is_none() { tx.execute("UPDATE quiz_state SET current=NULL, revealed=0 WHERE pad_id=?1", params![pad_id])?; }
        tx.execute("UPDATE pads SET last_edited=?2 WHERE id=?1", params![pad_id, ids::now_ms()])?;
        tx.commit()?;
        Ok(true)
    }
    /// Letzte Wertung zurücknehmen. false: es gab keine.
    /// Spielphase setzen; beim Beenden schließt eine offene Frage.
    pub fn quiz_set_phase(&self, pad_id: &str, phase: i64) -> Result<()> {
        self.c().execute("INSERT INTO quiz_state (pad_id, phase) VALUES (?1, ?2) ON CONFLICT(pad_id) DO UPDATE SET phase=?2, current=CASE WHEN ?2=1 THEN current ELSE NULL END, revealed=CASE WHEN ?2=1 THEN revealed ELSE 0 END", params![pad_id, phase])?;
        Ok(())
    }
    /// Neues Spiel mit denselben Fragen und Teams: Punkte, gespielte Felder und Buzzer-Drücke weg, zurück in die Vorbereitung.
    pub fn quiz_new_game(&self, pad_id: &str) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        for t in ["quiz_buzz", "quiz_buzz_open", "quiz_log"] { tx.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![pad_id])?; }
        tx.execute("UPDATE quiz_state SET current=NULL, revealed=0, phase=0 WHERE pad_id=?1", params![pad_id])?;
        tx.execute("UPDATE pads SET last_edited=?2 WHERE id=?1", params![pad_id, ids::now_ms()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn quiz_undo(&self, pad_id: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM quiz_log WHERE pad_id=?1 AND seq=(SELECT MAX(seq) FROM quiz_log WHERE pad_id=?1)", params![pad_id])? > 0)
    }
}

/// Buzzer-Messung: ein Uhrabgleich (beste von fünf Anfragen) eines Geräts, Zeiten in Millisekunden.
#[derive(Debug, Clone, Default)]
pub struct MessProbe { pub nr: i64, pub ts: f64, pub rtt: f64, pub rtt_med: f64, pub off: f64 }

/// Buzzer-Messung: Empfang des Signals und Druck eines Geräts in einer Runde. `t_*` in Gerätezeit, `ts_ein` in Serverzeit.
#[derive(Debug, Clone, Default)]
pub struct MessMarke { pub nr: i64, pub runde: i64, pub t_empfang: Option<f64>, pub t_druck: Option<f64>, pub ts_ein: Option<f64>, pub off: f64, pub rtt: f64 }

impl Store {
    /// Nummer des Geräts im Pad, bei Bedarf neu; None, wenn schon `max` Geräte mitmessen. Nach außen gehen nur Nummern.
    pub fn mess_geraet(&self, pad_id: &str, dev: &str, art: &str, max: i64) -> Result<Option<i64>> {
        let c = self.c();
        if let Some(nr) = c.query_row("SELECT nr FROM messung_geraet WHERE pad_id=?1 AND dev=?2", params![pad_id, dev], |r| r.get(0)).optional()? { return Ok(Some(nr)); }
        let n: i64 = c.query_row("SELECT COUNT(*) FROM messung_geraet WHERE pad_id=?1", params![pad_id], |r| r.get(0))?;
        if n >= max { return Ok(None); }
        c.execute("INSERT INTO messung_geraet (pad_id, dev, nr, art) VALUES (?1, ?2, ?3, ?4)", params![pad_id, dev, n + 1, art])?;
        Ok(Some(n + 1))
    }
    pub fn mess_geraete(&self, pad_id: &str) -> Result<Vec<(i64, String)>> {
        Ok(self.c().prepare("SELECT nr, art FROM messung_geraet WHERE pad_id=?1 ORDER BY nr")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }
    /// false, wenn das Pad schon `max` Abgleiche hat.
    pub fn mess_probe(&self, pad_id: &str, p: &MessProbe, max: i64) -> Result<bool> {
        let c = self.c();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM messung_probe WHERE pad_id=?1", params![pad_id], |r| r.get(0))?;
        if n >= max { return Ok(false); }
        c.execute("INSERT INTO messung_probe (pad_id, nr, ts, rtt, rtt_med, off) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![pad_id, p.nr, p.ts, p.rtt, p.rtt_med, p.off])?;
        Ok(true)
    }
    pub fn mess_proben(&self, pad_id: &str) -> Result<Vec<MessProbe>> {
        Ok(self.c().prepare("SELECT nr, ts, rtt, rtt_med, off FROM messung_probe WHERE pad_id=?1 ORDER BY nr, ts")?
            .query_map(params![pad_id], |r| Ok(MessProbe { nr: r.get(0)?, ts: r.get(1)?, rtt: r.get(2)?, rtt_med: r.get(3)?, off: r.get(4)? }))?.collect::<rusqlite::Result<_>>()?)
    }
    /// Neue Runde mit Serverzeit `ts`; None, wenn schon `max` Runden gelaufen sind.
    pub fn mess_signal(&self, pad_id: &str, ts: f64, max: i64) -> Result<Option<i64>> {
        let c = self.c();
        let runde: i64 = c.query_row("SELECT COALESCE(MAX(runde), 0) + 1 FROM messung_runde WHERE pad_id=?1", params![pad_id], |r| r.get(0))?;
        if runde > max { return Ok(None); }
        c.execute("INSERT INTO messung_runde (pad_id, runde, ts) VALUES (?1, ?2, ?3)", params![pad_id, runde, ts])?;
        Ok(Some(runde))
    }
    pub fn mess_runden(&self, pad_id: &str) -> Result<Vec<(i64, f64)>> {
        Ok(self.c().prepare("SELECT runde, ts FROM messung_runde WHERE pad_id=?1 ORDER BY runde")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }
    /// Empfang des Signals; der erste Empfang zählt. false, wenn es die Runde nicht gibt.
    pub fn mess_empfang(&self, pad_id: &str, nr: i64, runde: i64, t: f64, off: f64, rtt: f64) -> Result<bool> {
        let c = self.c();
        if c.query_row("SELECT 1 FROM messung_runde WHERE pad_id=?1 AND runde=?2", params![pad_id, runde], |_| Ok(())).optional()?.is_none() { return Ok(false); }
        c.execute("INSERT INTO messung_marke (pad_id, nr, runde, t_empfang, off, rtt) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                   ON CONFLICT(pad_id, nr, runde) DO UPDATE SET t_empfang=COALESCE(t_empfang, ?4)", params![pad_id, nr, runde, t, off, rtt])?;
        Ok(true)
    }
    /// Druck; je Gerät und Runde zählt nur der erste. false, wenn es die Runde nicht gibt.
    #[allow(clippy::too_many_arguments)] // eine Zeile der Tabelle, Spalte für Spalte
    pub fn mess_druck(&self, pad_id: &str, nr: i64, runde: i64, t: f64, ts_ein: f64, off: f64, rtt: f64) -> Result<bool> {
        let c = self.c();
        if c.query_row("SELECT 1 FROM messung_runde WHERE pad_id=?1 AND runde=?2", params![pad_id, runde], |_| Ok(())).optional()?.is_none() { return Ok(false); }
        c.execute("INSERT INTO messung_marke (pad_id, nr, runde, t_druck, ts_ein, off, rtt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                   ON CONFLICT(pad_id, nr, runde) DO UPDATE SET t_druck=?4, ts_ein=?5, off=?6, rtt=?7 WHERE t_druck IS NULL", params![pad_id, nr, runde, t, ts_ein, off, rtt])?;
        Ok(true)
    }
    pub fn mess_marken(&self, pad_id: &str) -> Result<Vec<MessMarke>> {
        Ok(self.c().prepare("SELECT nr, runde, t_empfang, t_druck, ts_ein, off, rtt FROM messung_marke WHERE pad_id=?1 ORDER BY runde, nr")?
            .query_map(params![pad_id], |r| Ok(MessMarke { nr: r.get(0)?, runde: r.get(1)?, t_empfang: r.get(2)?, t_druck: r.get(3)?, ts_ein: r.get(4)?, off: r.get(5)?, rtt: r.get(6)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn mess_loeschen(&self, pad_id: &str) -> Result<()> {
        let c = self.c();
        for t in MESS_TABELLEN { c.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![pad_id])?; }
        Ok(())
    }
}

pub const MESS_TABELLEN: [&str; 4] = ["messung_marke", "messung_runde", "messung_probe", "messung_geraet"];

/// Ein Buzzer-Druck: `t_press` ist die Druckzeit in Serverzeit (nach Uhrabgleich), `ts_ein` der Eingang beim Server.
/// `flagged`: Zeit war unplausibel, ersetzt durch Eingang minus halbe Laufzeit.
#[derive(Debug, Clone, Default)]
/// `reaktion`: Druckzeit minus Öffnen der Frage (ms); fehlt bei Drücken aus älteren Versionen.
pub struct Buzz { pub team: i64, pub t_press: f64, pub ts_ein: f64, pub flagged: bool, pub reaktion: Option<f64> }

impl Store {
    /// Neue Buzzer-Runde für ein Feld: alte Drücke dieses Felds weg, Öffnungszeit merken.
    pub fn quiz_buzz_start(&self, pad_id: &str, field: i64, ts: f64) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM quiz_buzz WHERE pad_id=?1 AND field=?2", params![pad_id, field])?;
        tx.execute("INSERT INTO quiz_buzz_open (pad_id, field, ts) VALUES (?1, ?2, ?3) ON CONFLICT(pad_id) DO UPDATE SET field=?2, ts=?3", params![pad_id, field, ts])?;
        tx.commit()?;
        Ok(())
    }
    /// Feld und Öffnungszeit der laufenden Buzzer-Runde.
    pub fn quiz_buzz_open(&self, pad_id: &str) -> Result<Option<(i64, f64)>> {
        Ok(self.c().query_row("SELECT field, ts FROM quiz_buzz_open WHERE pad_id=?1", params![pad_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
    }
    /// Ein Druck je Gerät und Feld; false, wenn das Gerät schon gedrückt hat oder das Feld voll ist.
    pub fn quiz_buzz_add(&self, pad_id: &str, field: i64, dev: &str, b: &Buzz, max: i64) -> Result<bool> {
        let c = self.c();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM quiz_buzz WHERE pad_id=?1 AND field=?2", params![pad_id, field], |r| r.get(0))?;
        if n >= max { return Ok(false); }
        Ok(c.execute("INSERT OR IGNORE INTO quiz_buzz (pad_id, field, dev, team, t_press, ts_ein, flagged, reaktion) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![pad_id, field, dev, b.team, b.t_press, b.ts_ein, b.flagged, b.reaktion])? > 0)
    }
    /// Team, für das ein Gerät in diesem Spiel schon gedrückt hat (ein neues Spiel leert die Drücke).
    pub fn quiz_buzz_team(&self, pad_id: &str, dev: &str) -> Result<Option<i64>> {
        Ok(self.c().query_row("SELECT team FROM quiz_buzz WHERE pad_id=?1 AND dev=?2 LIMIT 1", params![pad_id, dev], |r| r.get(0)).optional()?)
    }
    pub fn quiz_buzzes(&self, pad_id: &str, field: i64) -> Result<Vec<Buzz>> {
        Ok(self.c().prepare("SELECT team, t_press, ts_ein, flagged, reaktion FROM quiz_buzz WHERE pad_id=?1 AND field=?2 ORDER BY ts_ein")?
            .query_map(params![pad_id, field], |r| Ok(Buzz { team: r.get(0)?, t_press: r.get(1)?, ts_ein: r.get(2)?, flagged: r.get(3)?, reaktion: r.get(4)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Für die Statistik am Ende: je Feld und Team die schnellste sichere Reaktion (ms).
    pub fn quiz_reaktionen(&self, pad_id: &str) -> Result<Vec<(i64, f64)>> {
        Ok(self.c().prepare("SELECT team, MIN(reaktion) FROM quiz_buzz WHERE pad_id=?1 AND flagged=0 AND reaktion IS NOT NULL GROUP BY field, team")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
}
