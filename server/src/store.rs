//! Persistenz: SQLite mit Metadaten, Update-Log und Snapshots.
use crate::ids;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::{mpsc, Mutex}, time::{Duration, Instant}};

pub struct Store {
    conn: Mutex<Connection>,
    /// Schreib-Thread für Yjs-Updates und Wartung (eigene Verbindung, Transaktionen gebündelt)
    writer: mpsc::SyncSender<WriteOp>,
}

/// Aufträge an den Schreib-Thread. Reihenfolge bleibt erhalten.
pub enum WriteOp {
    Append { pad: String, seq: i64, ts: i64, author: Option<String>, bytes: Vec<u8> },
    Exec(Box<dyn FnOnce(&Connection) -> rusqlite::Result<()> + Send>),
    Flush(mpsc::Sender<()>),
}

/// Bündelt Updates: wartet nach dem ersten Auftrag höchstens 5 ms oder bis 512 Aufträge, schreibt alles in einer Transaktion.
fn writer_loop(conn: Connection, rx: mpsc::Receiver<WriteOp>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        let deadline = Instant::now() + Duration::from_millis(5);
        while batch.len() < 512 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { break; }
            match rx.recv_timeout(left) { Ok(op) => batch.push(op), Err(_) => break }
        }
        let mut flushes = Vec::new();
        let res: rusqlite::Result<()> = (|| {
            let tx = conn.unchecked_transaction()?;
            let mut touched: std::collections::HashMap<String, i64> = Default::default();
            {
                let mut ins = tx.prepare_cached(
                    "INSERT OR IGNORE INTO updates (pad_id, seq, ts, author_id, bytes) SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM pads WHERE id=?1)")?;
                for op in batch {
                    match op {
                        WriteOp::Append { pad, seq, ts, author, bytes } => { ins.execute(params![pad, seq, ts, author, bytes])?; touched.insert(pad, ts); }
                        WriteOp::Exec(f) => { if let Err(e) = f(&tx) { tracing::warn!("Schreib-Thread, Wartung: {e}"); } }
                        WriteOp::Flush(s) => flushes.push(s),
                    }
                }
            }
            for (pad, ts) in touched { tx.execute("UPDATE pads SET last_edited=?2 WHERE id=?1 AND last_edited < ?2", params![pad, ts])?; }
            tx.commit()
        })();
        if let Err(e) = res { tracing::error!("Schreib-Thread: Updates nicht gespeichert: {e}"); }
        for s in flushes { let _ = s.send(()); }
    }
}

#[derive(Clone, Debug)]
pub struct Pad {
    pub id: String,
    pub group_id: Option<String>,
    pub readonly_id: String,
    pub created: i64,
    pub last_edited: i64,
}

#[derive(Debug, Clone, Default)]
pub struct QuizState { pub title: String, pub current: Option<i64>, pub revealed: bool }
#[derive(Debug, Clone, serde::Serialize)]
pub struct QuizField { pub id: i64, pub cat: String, pub points: i64, pub question: String, pub answer: String }

#[derive(Clone, Debug, serde::Serialize)]
pub struct Idea {
    pub id: i64,
    pub text: String,
    pub created: i64,
    pub hidden: bool,
    pub done: bool,
    pub votes: i64,
}

#[derive(Clone, Debug)]
pub struct Author {
    pub id: String,
    pub mapper: Option<String>,
    pub name: String,
    pub avatar: Option<String>,
}

/// Ein freies Pad mit allem, was die Aufbewahrungsregel braucht (Zeiten in ms).
#[derive(Clone, Debug)]
pub struct RetentionRow {
    pub id: String,
    pub created: i64,
    pub last_edited: i64,
    pub last_seen: Option<i64>,
    pub keep_until: Option<i64>,
    pub notice_sent: Option<i64>,
    pub delete_after: Option<i64>,
    pub owner_id: Option<String>,
    pub email: Option<String>,
    pub owner_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub group_id: String,
    pub author_id: String,
    pub valid_until: i64,
}

#[derive(Debug)]
pub enum CreatePadError {
    Exists,
    NoSuchGroup,
    Other(anyhow::Error),
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS groups (id TEXT PRIMARY KEY, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS pads (
  id TEXT PRIMARY KEY, group_id TEXT REFERENCES groups(id), readonly_id TEXT UNIQUE NOT NULL,
  created INTEGER NOT NULL, last_edited INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS authors (
  id TEXT PRIMARY KEY, mapper TEXT UNIQUE, name TEXT NOT NULL, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY, group_id TEXT NOT NULL, author_id TEXT NOT NULL, valid_until INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS sessions_group ON sessions(group_id);
CREATE TABLE IF NOT EXISTS logins (token TEXT PRIMARY KEY, author_id TEXT NOT NULL, created INTEGER NOT NULL, valid_until INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS guest_tokens (token TEXT PRIMARY KEY, author_id TEXT NOT NULL, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS updates (
  pad_id TEXT NOT NULL, seq INTEGER NOT NULL, ts INTEGER NOT NULL, author_id TEXT, bytes BLOB NOT NULL,
  PRIMARY KEY (pad_id, seq));
CREATE TABLE IF NOT EXISTS snapshots (
  pad_id TEXT NOT NULL, ts INTEGER NOT NULL, seq INTEGER NOT NULL, label TEXT NOT NULL,
  state BLOB NOT NULL, html TEXT NOT NULL, PRIMARY KEY (pad_id, ts));
CREATE TABLE IF NOT EXISTS ideas (
  pad_id TEXT NOT NULL, id INTEGER NOT NULL, author_id TEXT NOT NULL, text TEXT NOT NULL, created INTEGER NOT NULL,
  hidden INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (pad_id, id));
CREATE TABLE IF NOT EXISTS idea_votes (pad_id TEXT NOT NULL, idea_id INTEGER NOT NULL, author_id TEXT NOT NULL, PRIMARY KEY (pad_id, idea_id, author_id));
CREATE TABLE IF NOT EXISTS idea_prompts (pad_id TEXT PRIMARY KEY, prompt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS quiz_state (pad_id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', current INTEGER, revealed INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS quiz_fields (pad_id TEXT NOT NULL, id INTEGER NOT NULL, cat TEXT NOT NULL, points INTEGER NOT NULL, question TEXT NOT NULL, answer TEXT NOT NULL, PRIMARY KEY (pad_id, id));
CREATE TABLE IF NOT EXISTS quiz_teams (pad_id TEXT NOT NULL, id INTEGER NOT NULL, name TEXT NOT NULL, PRIMARY KEY (pad_id, id));
CREATE TABLE IF NOT EXISTS quiz_log (pad_id TEXT NOT NULL, seq INTEGER NOT NULL, field INTEGER NOT NULL, team INTEGER, delta INTEGER NOT NULL, PRIMARY KEY (pad_id, seq));
CREATE TABLE IF NOT EXISTS messung_geraet (pad_id TEXT NOT NULL, dev TEXT NOT NULL, nr INTEGER NOT NULL, art TEXT NOT NULL, PRIMARY KEY (pad_id, dev));
CREATE TABLE IF NOT EXISTS messung_probe (pad_id TEXT NOT NULL, nr INTEGER NOT NULL, ts REAL NOT NULL, rtt REAL NOT NULL, rtt_med REAL NOT NULL, off REAL NOT NULL);
CREATE INDEX IF NOT EXISTS messung_probe_pad ON messung_probe (pad_id);
CREATE TABLE IF NOT EXISTS messung_runde (pad_id TEXT NOT NULL, runde INTEGER NOT NULL, ts REAL NOT NULL, PRIMARY KEY (pad_id, runde));
CREATE TABLE IF NOT EXISTS messung_marke (pad_id TEXT NOT NULL, nr INTEGER NOT NULL, runde INTEGER NOT NULL, t_empfang REAL, t_druck REAL, ts_ein REAL, off REAL NOT NULL, rtt REAL NOT NULL, PRIMARY KEY (pad_id, nr, runde));
CREATE TABLE IF NOT EXISTS lti_links (link_key TEXT PRIMARY KEY, pad_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pad_moderators (pad_id TEXT NOT NULL, author_id TEXT NOT NULL, PRIMARY KEY (pad_id, author_id));
"#;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(SCHEMA)?;
        // Schema-Ergänzung 24.10.2026: Moodle-Herkunft für Profilbilder
        let has_col: bool = conn.prepare("SELECT 1 FROM pragma_table_info('authors') WHERE name='moodle_url'")?.exists([])?;
        if !has_col { conn.execute_batch("ALTER TABLE authors ADD COLUMN moodle_url TEXT; ALTER TABLE authors ADD COLUMN moodle_user TEXT;")?; }
        // Schema-Ergänzung 24.10.2026: Pad-Typ (leer = Text, "board" = Zeichenbrett)
        let has_kind: bool = conn.prepare("SELECT 1 FROM pragma_table_info('pads') WHERE name='kind'")?.exists([])?;
        if !has_kind { conn.execute_batch("ALTER TABLE pads ADD COLUMN kind TEXT;")?; }
        // Schema-Ergänzung 24.10.2026: Besitz und Aufbewahrung freier Pads, Mailadresse aus Moodle
        let has_owner: bool = conn.prepare("SELECT 1 FROM pragma_table_info('pads') WHERE name='owner_id'")?.exists([])?;
        if !has_owner {
            conn.execute_batch("ALTER TABLE pads ADD COLUMN owner_id TEXT; ALTER TABLE pads ADD COLUMN last_seen INTEGER; ALTER TABLE pads ADD COLUMN keep_until INTEGER;
                ALTER TABLE pads ADD COLUMN notice_sent INTEGER; ALTER TABLE pads ADD COLUMN delete_after INTEGER; ALTER TABLE pads ADD COLUMN extend_token TEXT;
                ALTER TABLE authors ADD COLUMN email TEXT; CREATE INDEX IF NOT EXISTS pads_extend ON pads(extend_token);")?;
        }
        // Schema-Ergänzung 26.10.2026: Admin-Recht einer Anmeldung über Moodle
        let has_admin: bool = conn.prepare("SELECT 1 FROM pragma_table_info('logins') WHERE name='admin'")?.exists([])?;
        if !has_admin { conn.execute_batch("ALTER TABLE logins ADD COLUMN admin INTEGER NOT NULL DEFAULT 0;")?; }
        let wconn = Connection::open(path)?;
        wconn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        let (tx, rx) = mpsc::sync_channel::<WriteOp>(65_536);
        std::thread::Builder::new().name("sqlite-writer".into()).spawn(move || writer_loop(wconn, rx))?;
        Ok(Store { conn: Mutex::new(conn), writer: tx })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Gruppen
    pub fn create_group(&self) -> Result<String> {
        let id = ids::group_id();
        self.c().execute("INSERT INTO groups (id, created) VALUES (?1, ?2)", params![id, ids::now_ms()])?;
        Ok(id)
    }
    pub fn group_exists(&self, id: &str) -> Result<bool> {
        Ok(self.c().query_row("SELECT 1 FROM groups WHERE id=?1", params![id], |_| Ok(())).optional()?.is_some())
    }
    /// Löscht Gruppe, ihre Pads samt Daten und ihre Sessions. Liefert die gelöschten Pad-Kennungen.
    pub fn delete_group(&self, id: &str) -> Result<Vec<String>> {
        let mut conn = self.c();
        let c = conn.transaction()?;
        let pads: Vec<String> = c
            .prepare("SELECT id FROM pads WHERE group_id=?1")?
            .query_map(params![id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for p in &pads {
            Self::delete_pad_in(&c, p)?;
        }
        c.execute("DELETE FROM sessions WHERE group_id=?1", params![id])?;
        c.execute("DELETE FROM groups WHERE id=?1", params![id])?;
        c.commit()?;
        Ok(pads)
    }

    // Pads
    pub fn create_pad(&self, id: &str, group_id: Option<&str>) -> std::result::Result<Pad, CreatePadError> {
        let c = self.c();
        if let Some(g) = group_id {
            let ok = c
                .query_row("SELECT 1 FROM groups WHERE id=?1", params![g], |_| Ok(()))
                .optional()
                .map_err(|e| CreatePadError::Other(e.into()))?
                .is_some();
            if !ok {
                return Err(CreatePadError::NoSuchGroup);
            }
        }
        let now = ids::now_ms();
        let ro = ids::readonly_id();
        match c.execute(
            "INSERT INTO pads (id, group_id, readonly_id, created, last_edited) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![id, group_id, ro, now],
        ) {
            Ok(_) => Ok(Pad { id: id.to_string(), group_id: group_id.map(str::to_string), readonly_id: ro, created: now, last_edited: now }),
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => Err(CreatePadError::Exists),
            Err(e) => Err(CreatePadError::Other(e.into())),
        }
    }
    pub fn get_pad(&self, id: &str) -> Result<Option<Pad>> {
        Ok(self
            .c()
            .query_row(
                "SELECT id, group_id, readonly_id, created, last_edited FROM pads WHERE id=?1",
                params![id],
                |r| Ok(Pad { id: r.get(0)?, group_id: r.get(1)?, readonly_id: r.get(2)?, created: r.get(3)?, last_edited: r.get(4)? }),
            )
            .optional()?)
    }
    pub fn pad_by_readonly(&self, ro: &str) -> Result<Option<Pad>> {
        Ok(self
            .c()
            .query_row(
                "SELECT id, group_id, readonly_id, created, last_edited FROM pads WHERE readonly_id=?1",
                params![ro],
                |r| Ok(Pad { id: r.get(0)?, group_id: r.get(1)?, readonly_id: r.get(2)?, created: r.get(3)?, last_edited: r.get(4)? }),
            )
            .optional()?)
    }
    fn delete_pad_in(c: &Connection, id: &str) -> Result<bool> {
        c.execute("DELETE FROM updates WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM idea_votes WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM ideas WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM idea_prompts WHERE pad_id=?1", params![id])?;
        for t in ["quiz_log", "quiz_teams", "quiz_fields", "quiz_state"] { c.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![id])?; }
        for t in MESS_TABELLEN { c.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![id])?; }
        c.execute("DELETE FROM pad_moderators WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM lti_links WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM snapshots WHERE pad_id=?1", params![id])?;
        Ok(c.execute("DELETE FROM pads WHERE id=?1", params![id])? > 0)
    }
    pub fn delete_pad(&self, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let r = Self::delete_pad_in(&tx, id)?;
        tx.commit()?;
        Ok(r)
    }
    // Pad-Typ
    pub fn set_kind(&self, pad_id: &str, kind: &str) -> Result<()> {
        self.c().execute("UPDATE pads SET kind=?2 WHERE id=?1", params![pad_id, kind])?;
        Ok(())
    }
    fn kind(&self, pad_id: &str) -> Option<String> {
        self.c().query_row("SELECT kind FROM pads WHERE id=?1", params![pad_id], |r| r.get::<_, Option<String>>(0)).ok().flatten()
    }
    pub fn is_board(&self, pad_id: &str) -> bool { self.kind(pad_id).as_deref() == Some("board") }
    pub fn is_ideas(&self, pad_id: &str) -> bool { self.kind(pad_id).as_deref() == Some("ideas") }
    pub fn is_quiz(&self, pad_id: &str) -> bool { self.kind(pad_id).as_deref() == Some("quiz") }

    // Ideensammlung: Beiträge ohne Namen, eine Stimme je Person und Beitrag
    pub fn ideas(&self, pad_id: &str) -> Result<Vec<Idea>> {
        Ok(self.c().prepare(
            "SELECT i.id, i.text, i.created, i.hidden, i.done, (SELECT COUNT(*) FROM idea_votes v WHERE v.pad_id=i.pad_id AND v.idea_id=i.id)
             FROM ideas i WHERE i.pad_id=?1 ORDER BY i.id")?
            .query_map(params![pad_id], |r| Ok(Idea { id: r.get(0)?, text: r.get(1)?, created: r.get(2)?, hidden: r.get(3)?, done: r.get(4)?, votes: r.get(5)? }))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn my_votes(&self, pad_id: &str, author_id: &str) -> Result<Vec<i64>> {
        Ok(self.c().prepare("SELECT idea_id FROM idea_votes WHERE pad_id=?1 AND author_id=?2")?
            .query_map(params![pad_id, author_id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?)
    }
    pub fn mine(&self, pad_id: &str, author_id: &str) -> Result<Vec<i64>> {
        Ok(self.c().prepare("SELECT id FROM ideas WHERE pad_id=?1 AND author_id=?2")?
            .query_map(params![pad_id, author_id], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?)
    }
    /// Legt einen Beitrag an, solange die Sammlung (`max_total`) und die Person (`max_own`) noch Platz haben.
    pub fn add_idea(&self, pad_id: &str, author_id: &str, text: &str, max_total: i64, max_own: i64) -> Result<Option<i64>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let (total, own): (i64, i64) = tx.query_row("SELECT COUNT(*), COALESCE(SUM(author_id=?2), 0) FROM ideas WHERE pad_id=?1", params![pad_id, author_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        if total >= max_total || own >= max_own { return Ok(None); }
        let now = ids::now_ms();
        let id: i64 = tx.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM ideas WHERE pad_id=?1", params![pad_id], |r| r.get(0))?;
        tx.execute("INSERT INTO ideas (pad_id, id, author_id, text, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![pad_id, id, author_id, text, now])?;
        tx.execute("UPDATE pads SET last_edited=?2 WHERE id=?1", params![pad_id, now])?;
        tx.commit()?;
        Ok(Some(id))
    }
    /// Stimme setzen oder zurücknehmen; nur für sichtbare, offene Beiträge. false: Beitrag nicht abstimmbar.
    pub fn vote(&self, pad_id: &str, idea_id: i64, author_id: &str, on: bool) -> Result<bool> {
        let c = self.c();
        let open = c.query_row("SELECT 1 FROM ideas WHERE pad_id=?1 AND id=?2 AND hidden=0 AND done=0", params![pad_id, idea_id], |_| Ok(())).optional()?.is_some();
        if !open { return Ok(false); }
        if on { c.execute("INSERT OR IGNORE INTO idea_votes (pad_id, idea_id, author_id) VALUES (?1, ?2, ?3)", params![pad_id, idea_id, author_id])?; }
        else { c.execute("DELETE FROM idea_votes WHERE pad_id=?1 AND idea_id=?2 AND author_id=?3", params![pad_id, idea_id, author_id])?; }
        Ok(true)
    }
    pub fn moderate_idea(&self, pad_id: &str, idea_id: i64, hidden: Option<bool>, done: Option<bool>) -> Result<bool> {
        let c = self.c();
        let mut n = 0;
        if let Some(h) = hidden { n = c.execute("UPDATE ideas SET hidden=?3 WHERE pad_id=?1 AND id=?2", params![pad_id, idea_id, h])?; }
        if let Some(d) = done { n = c.execute("UPDATE ideas SET done=?3 WHERE pad_id=?1 AND id=?2", params![pad_id, idea_id, d])?; }
        Ok(n > 0)
    }

    // Quiz-Raster: Fragen und Antworten nur hier, nie im Yjs-Dokument
    pub fn quiz_state(&self, pad_id: &str) -> Result<QuizState> {
        Ok(self.c().query_row("SELECT title, current, revealed FROM quiz_state WHERE pad_id=?1", params![pad_id],
            |r| Ok(QuizState { title: r.get(0)?, current: r.get(1)?, revealed: r.get(2)? })).optional()?.unwrap_or_default())
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
    pub fn quiz_replace(&self, pad_id: &str, title: &str, fields: &[QuizField], teams: &[String]) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        for t in ["quiz_log", "quiz_teams", "quiz_fields"] { tx.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![pad_id])?; }
        for f in fields { tx.execute("INSERT INTO quiz_fields (pad_id, id, cat, points, question, answer) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![pad_id, f.id, f.cat, f.points, f.question, f.answer])?; }
        for (i, t) in teams.iter().enumerate() { tx.execute("INSERT INTO quiz_teams (pad_id, id, name) VALUES (?1, ?2, ?3)", params![pad_id, i as i64 + 1, t])?; }
        tx.execute("INSERT INTO quiz_state (pad_id, title, current, revealed) VALUES (?1, ?2, NULL, 0) ON CONFLICT(pad_id) DO UPDATE SET title=?2, current=NULL, revealed=0", params![pad_id, title])?;
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
    pub fn quiz_undo(&self, pad_id: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM quiz_log WHERE pad_id=?1 AND seq=(SELECT MAX(seq) FROM quiz_log WHERE pad_id=?1)", params![pad_id])? > 0)
    }
    // Aktivitäten aus Moodle (LTI): je Ressourcen-Link eine eigene Gruppe mit einem Pad
    pub fn pad_for_link(&self, link_key: &str) -> Result<Option<String>> {
        Ok(self.c().query_row("SELECT l.pad_id FROM lti_links l JOIN pads p ON p.id=l.pad_id WHERE l.link_key=?1", params![link_key], |r| r.get(0)).optional()?)
    }
    /// Legt Gruppe und Pad für einen Ressourcen-Link an. Liefert die Pad-Kennung.
    pub fn create_link_pad(&self, link_key: &str, kind: &str) -> Result<String> {
        let group = ids::group_id();
        let pad = format!("{group}$aktivitaet");
        let mut c = self.c();
        let tx = c.transaction()?;
        let now = ids::now_ms();
        tx.execute("INSERT INTO groups (id, created) VALUES (?1, ?2)", params![group, now])?;
        tx.execute("INSERT INTO pads (id, group_id, readonly_id, created, last_edited, kind) VALUES (?1, ?2, ?3, ?4, ?4, ?5)", params![pad, group, ids::readonly_id(), now, kind])?;
        tx.execute("INSERT OR REPLACE INTO lti_links (link_key, pad_id) VALUES (?1, ?2)", params![link_key, pad])?;
        tx.commit()?;
        Ok(pad)
    }
    pub fn add_moderator(&self, pad_id: &str, author_id: &str) -> Result<()> {
        self.c().execute("INSERT OR IGNORE INTO pad_moderators (pad_id, author_id) VALUES (?1, ?2)", params![pad_id, author_id])?;
        Ok(())
    }
    pub fn is_moderator(&self, pad_id: &str, author_id: &str) -> bool {
        self.c().query_row("SELECT 1 FROM pad_moderators WHERE pad_id=?1 AND author_id=?2", params![pad_id, author_id], |_| Ok(())).optional().ok().flatten().is_some()
    }
    pub fn idea_prompt(&self, pad_id: &str) -> Result<String> {
        Ok(self.c().query_row("SELECT prompt FROM idea_prompts WHERE pad_id=?1", params![pad_id], |r| r.get(0)).optional()?.unwrap_or_default())
    }
    pub fn set_idea_prompt(&self, pad_id: &str, prompt: &str) -> Result<()> {
        self.c().execute("INSERT INTO idea_prompts (pad_id, prompt) VALUES (?1, ?2) ON CONFLICT(pad_id) DO UPDATE SET prompt=excluded.prompt", params![pad_id, prompt])?;
        Ok(())
    }

    // Besitz und Aufbewahrung freier Pads
    pub fn set_owner(&self, pad_id: &str, author_id: &str) -> Result<()> {
        self.c().execute("UPDATE pads SET owner_id=?2 WHERE id=?1 AND owner_id IS NULL", params![pad_id, author_id])?;
        Ok(())
    }
    pub fn owner_of(&self, pad_id: &str) -> Result<Option<String>> {
        Ok(self.c().query_row("SELECT owner_id FROM pads WHERE id=?1", params![pad_id], |r| r.get(0)).optional()?.flatten())
    }
    /// Aufruf eines Pads zählt als Aktivität; höchstens stündlich geschrieben.
    pub fn touch_seen(&self, pad_id: &str, now: i64) -> Result<()> {
        self.c().execute("UPDATE pads SET last_seen=?2 WHERE id=?1 AND (last_seen IS NULL OR last_seen < ?2 - 3600000)", params![pad_id, now])?;
        Ok(())
    }
    pub fn retention_rows(&self) -> Result<Vec<RetentionRow>> {
        let c = self.c();
        let mut st = c.prepare("SELECT p.id, p.created, p.last_edited, p.last_seen, p.keep_until, p.notice_sent, p.delete_after, p.owner_id, a.email, a.name
            FROM pads p LEFT JOIN authors a ON a.id=p.owner_id WHERE p.group_id IS NULL")?;
        let rows = st.query_map([], |r| Ok(RetentionRow {
            id: r.get(0)?, created: r.get(1)?, last_edited: r.get(2)?, last_seen: r.get(3)?, keep_until: r.get(4)?,
            notice_sent: r.get(5)?, delete_after: r.get(6)?, owner_id: r.get(7)?, email: r.get(8)?, owner_name: r.get(9)?,
        }))?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    pub fn retention_row(&self, pad_id: &str) -> Result<Option<RetentionRow>> {
        Ok(self.retention_rows()?.into_iter().find(|r| r.id == pad_id))
    }
    pub fn set_notice(&self, pad_id: &str, sent: i64, delete_after: i64, token: &str) -> Result<()> {
        self.c().execute("UPDATE pads SET notice_sent=?2, delete_after=?3, extend_token=?4 WHERE id=?1", params![pad_id, sent, delete_after, token])?;
        Ok(())
    }
    pub fn clear_notice(&self, pad_id: &str) -> Result<()> {
        self.c().execute("UPDATE pads SET notice_sent=NULL, delete_after=NULL, extend_token=NULL WHERE id=?1", params![pad_id])?;
        Ok(())
    }
    pub fn extend(&self, pad_id: &str, keep_until: i64) -> Result<()> {
        self.c().execute("UPDATE pads SET keep_until=?2, notice_sent=NULL, delete_after=NULL, extend_token=NULL WHERE id=?1", params![pad_id, keep_until])?;
        Ok(())
    }
    pub fn pad_by_extend_token(&self, token: &str) -> Result<Option<String>> {
        Ok(self.c().query_row("SELECT id FROM pads WHERE extend_token=?1", params![token], |r| r.get(0)).optional()?)
    }
    pub fn set_author_email(&self, author_id: &str, email: Option<&str>) -> Result<()> {
        self.c().execute("UPDATE authors SET email=?2 WHERE id=?1", params![author_id, email])?;
        Ok(())
    }

    pub fn list_pads(&self) -> Result<Vec<String>> {
        Ok(self.c().prepare("SELECT id FROM pads ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?)
    }
    pub fn touch_pad(&self, id: &str, ts: i64) -> Result<()> {
        self.c().execute("UPDATE pads SET last_edited=?2 WHERE id=?1", params![id, ts])?;
        Ok(())
    }

    // Autoren
    pub fn create_author(&self, name: &str, mapper: Option<&str>) -> Result<Author> { self.create_author_from(name, mapper, None) }

    /// Autor anlegen oder holen. Mit Moodle-Adresse wird der Mapper je Moodle getrennt
    /// (gleiche Nutzer-ID in zwei Moodles = zwei Personen) und das Profilbild ableitbar.
    pub fn create_author_from(&self, name: &str, mapper: Option<&str>, moodle: Option<&str>) -> Result<Author> {
        let c = self.c();
        let key = match (mapper, moodle) { (Some(m), Some(u)) => Some(format!("{u}#{m}")), (Some(m), None) => Some(m.to_string()), _ => None };
        if let Some(k) = &key {
            if let Some(a) = c.query_row("SELECT id, mapper, name, moodle_url, moodle_user FROM authors WHERE mapper=?1", params![k], |r| {
                Ok(Author { id: r.get(0)?, mapper: r.get(1)?, name: r.get(2)?, avatar: avatar_url(r.get(3)?, r.get(4)?) })
            }).optional()? {
                if a.name != name && !name.is_empty() { c.execute("UPDATE authors SET name=?2 WHERE id=?1", params![a.id, name])?; }
                return Ok(Author { name: if name.is_empty() { a.name } else { name.to_string() }, ..a });
            }
        }
        let id = ids::author_id();
        let (mu, mus) = match (mapper, moodle) { (Some(m), Some(u)) => (Some(u.to_string()), Some(m.to_string())), _ => (None, None) };
        c.execute("INSERT INTO authors (id, mapper, name, created, moodle_url, moodle_user) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![id, key, name, ids::now_ms(), mu, mus])?;
        Ok(Author { id, mapper: key, name: name.to_string(), avatar: avatar_url(mu, mus) })
    }
    pub fn create_guest_token(&self, author_id: &str) -> Result<String> {
        let token = format!("{}{}", ids::random16(), ids::random16());
        self.c().execute("INSERT INTO guest_tokens (token, author_id, created) VALUES (?1, ?2, ?3)", params![token, author_id, ids::now_ms()])?;
        Ok(token)
    }
    pub fn author_by_guest_token(&self, token: &str) -> Result<Option<Author>> {
        Ok(self.c().query_row(
            "SELECT a.id, a.mapper, a.name, a.moodle_url, a.moodle_user FROM guest_tokens t JOIN authors a ON a.id=t.author_id WHERE t.token=?1",
            params![token], |r| Ok(Author { id: r.get(0)?, mapper: r.get(1)?, name: r.get(2)?, avatar: avatar_url(r.get(3)?, r.get(4)?) })).optional()?)
    }
    /// Nur Gastautoren (ohne Moodle-Mapper) dürfen ihren Namen selbst setzen.
    pub fn rename_guest(&self, id: &str, name: &str) -> Result<bool> {
        Ok(self.c().execute("UPDATE authors SET name=?2 WHERE id=?1 AND mapper IS NULL", params![id, name])? > 0)
    }
    pub fn get_author(&self, id: &str) -> Result<Option<Author>> {
        Ok(self
            .c()
            .query_row("SELECT id, mapper, name, moodle_url, moodle_user FROM authors WHERE id=?1", params![id], |r| {
                Ok(Author { id: r.get(0)?, mapper: r.get(1)?, name: r.get(2)?, avatar: avatar_url(r.get(3)?, r.get(4)?) })
            })
            .optional()?)
    }

    // Sessions
    pub fn create_session(&self, group_id: &str, author_id: &str, valid_until: i64) -> Result<String> {
        let id = ids::session_id();
        self.c().execute(
            "INSERT INTO sessions (id, group_id, author_id, valid_until) VALUES (?1, ?2, ?3, ?4)",
            params![id, group_id, author_id, valid_until],
        )?;
        Ok(id)
    }
    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        Ok(self
            .c()
            .query_row("SELECT id, group_id, author_id, valid_until FROM sessions WHERE id=?1", params![id], |r| {
                Ok(Session { id: r.get(0)?, group_id: r.get(1)?, author_id: r.get(2)?, valid_until: r.get(3)? })
            })
            .optional()?)
    }
    pub fn delete_session(&self, id: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM sessions WHERE id=?1", params![id])? > 0)
    }
    pub fn purge_expired_sessions(&self, now_s: i64) -> Result<usize> {
        let c = self.c();
        let n = c.execute("DELETE FROM sessions WHERE valid_until < ?1", params![now_s])?;
        Ok(n + c.execute("DELETE FROM logins WHERE valid_until < ?1", params![now_s])?)
    }

    // Anmeldungen über Moodle (LTI)
    pub fn create_login(&self, author_id: &str, valid_until: i64, admin: bool) -> Result<String> {
        let token = format!("{}{}{}", ids::random16(), ids::random16(), ids::random16());
        self.c().execute("INSERT INTO logins (token, author_id, created, valid_until, admin) VALUES (?1, ?2, ?3, ?4, ?5)", params![token, author_id, ids::now_ms(), valid_until, admin as i64])?;
        Ok(token)
    }
    pub fn author_by_login(&self, token: &str, now_s: i64) -> Result<Option<Author>> {
        Ok(self.c().query_row(
            "SELECT a.id, a.mapper, a.name, a.moodle_url, a.moodle_user FROM logins l JOIN authors a ON a.id=l.author_id WHERE l.token=?1 AND l.valid_until > ?2",
            params![token, now_s], |r| Ok(Author { id: r.get(0)?, mapper: r.get(1)?, name: r.get(2)?, avatar: avatar_url(r.get(3)?, r.get(4)?) })).optional()?)
    }
    pub fn login_is_admin(&self, token: &str, now_s: i64) -> Result<bool> {
        Ok(self.c().query_row("SELECT admin FROM logins WHERE token=?1 AND valid_until > ?2", params![token, now_s], |r| r.get::<_, i64>(0)).optional()?.unwrap_or(0) == 1)
    }
    pub fn delete_login(&self, token: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM logins WHERE token=?1", params![token])? > 0)
    }

    // Einstellungen (Erscheinungsbild)
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self.c().query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0)).optional()?)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.c().execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        Ok(())
    }

    // Update-Log und Snapshots
    /// Hängt ein Update an; `None`, wenn das Pad nicht (mehr) existiert.
    /// Update an den Schreib-Thread geben (Nummer vergibt der Raum). Blockiert nur, wenn 65.536 Aufträge warten.
    pub fn queue_update(&self, pad_id: &str, seq: i64, bytes: &[u8], author: Option<&str>) {
        let op = WriteOp::Append { pad: pad_id.to_string(), seq, ts: ids::now_ms(), author: author.map(str::to_string), bytes: bytes.to_vec() };
        if self.writer.send(op).is_err() { tracing::error!(pad = %pad_id, "Schreib-Thread beendet, Update verloren"); }
    }
    /// Wartet, bis alle bisher übergebenen Aufträge geschrieben sind (vor dem Laden eines Pads, beim Beenden).
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.writer.send(WriteOp::Flush(tx)).is_ok() { let _ = rx.recv_timeout(Duration::from_secs(30)); }
    }
    /// Altbestand zusammenführen: Einzelschritte vor `cutoff_ms`, die ein Vollstand abdeckt, löschen;
    /// Vollstände vor `cutoff_ms` auf den letzten je Tag ausdünnen. Läuft im Schreib-Thread.
    pub fn compact(&self, cutoff_ms: i64) {
        let job = move |c: &Connection| -> rusqlite::Result<()> {
            let ups = c.execute(
                "DELETE FROM updates WHERE ts < ?1 AND seq <= COALESCE((SELECT MAX(s.seq) FROM snapshots s WHERE s.pad_id = updates.pad_id AND s.ts < ?1), -1)",
                params![cutoff_ms])?;
            let snaps = c.execute(
                "DELETE FROM snapshots WHERE ts < ?1 AND ts NOT IN (SELECT MAX(s2.ts) FROM snapshots s2 WHERE s2.pad_id = snapshots.pad_id AND s2.ts < ?1 GROUP BY s2.ts / 86400000)",
                params![cutoff_ms])?;
            tracing::info!("Zusammenführen: {ups} Einzelschritte und {snaps} Vollstände vor der Frist entfernt");
            Ok(())
        };
        let _ = self.writer.send(WriteOp::Exec(Box::new(job)));
    }
    pub fn updates_since(&self, pad_id: &str, seq: i64) -> Result<Vec<(i64, Vec<u8>)>> {
        Ok(self
            .c()
            .prepare("SELECT seq, bytes FROM updates WHERE pad_id=?1 AND seq>?2 ORDER BY seq")?
            .query_map(params![pad_id, seq], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }
    /// Alle Updates eines Pads (seq, ts, author, bytes) für die Zeitleiste.
    pub fn updates_all(&self, pad_id: &str) -> Result<Vec<(i64, i64, Option<String>, Vec<u8>)>> {
        Ok(self
            .c()
            .prepare("SELECT seq, ts, author_id, bytes FROM updates WHERE pad_id=?1 ORDER BY seq")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }
    /// Alle Snapshots eines Pads (ts, seq, label, state).
    pub fn snapshots_all(&self, pad_id: &str) -> Result<Vec<(i64, i64, String, Vec<u8>)>> {
        Ok(self
            .c()
            .prepare("SELECT ts, seq, label, state FROM snapshots WHERE pad_id=?1 ORDER BY ts")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn update_count(&self, pad_id: &str) -> Result<i64> {
        Ok(self.c().query_row("SELECT COALESCE(MAX(seq),0) FROM updates WHERE pad_id=?1", params![pad_id], |r| r.get(0))?)
    }
    /// Speichert einen Snapshot; `false`, wenn das Pad nicht (mehr) existiert.
    pub fn save_snapshot(&self, pad_id: &str, seq: i64, label: &str, state: &[u8], html: &str) -> Result<bool> {
        let n = self.c().execute(
            "INSERT OR REPLACE INTO snapshots (pad_id, ts, seq, label, state, html)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6 WHERE EXISTS (SELECT 1 FROM pads WHERE id=?1)",
            params![pad_id, ids::now_ms(), seq, label, state, html],
        )?;
        Ok(n > 0)
    }
    pub fn latest_snapshot(&self, pad_id: &str) -> Result<Option<(i64, Vec<u8>)>> {
        Ok(self
            .c()
            .query_row("SELECT seq, state FROM snapshots WHERE pad_id=?1 ORDER BY ts DESC LIMIT 1", params![pad_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?)
    }
    pub fn list_snapshots(&self, pad_id: &str) -> Result<Vec<(i64, String)>> {
        Ok(self
            .c()
            .prepare("SELECT ts, label FROM snapshots WHERE pad_id=?1 ORDER BY ts")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }
    pub fn snapshot_html(&self, pad_id: &str, ts: i64) -> Result<Option<String>> {
        Ok(self.c().query_row("SELECT html FROM snapshots WHERE pad_id=?1 AND ts=?2", params![pad_id, ts], |r| r.get(0)).optional()?)
    }
    pub fn counts(&self) -> Result<(i64, i64, i64)> {
        let c = self.c();
        let pads: i64 = c.query_row("SELECT COUNT(*) FROM pads", [], |r| r.get(0))?;
        let groups: i64 = c.query_row("SELECT COUNT(*) FROM groups", [], |r| r.get(0))?;
        let authors: i64 = c.query_row("SELECT COUNT(*) FROM authors", [], |r| r.get(0))?;
        Ok((pads, groups, authors))
    }
}

impl Store {
    // ---- Import aus Etherpad (Kennungen werden übernommen) ----
    pub fn import_group(&self, id: &str) -> Result<()> {
        self.c().execute("INSERT OR IGNORE INTO groups (id, created) VALUES (?1, ?2)", params![id, ids::now_ms()])?;
        Ok(())
    }
    pub fn import_pad(&self, id: &str, group_id: Option<&str>, readonly_id: &str, created: i64, last_edited: i64) -> Result<()> {
        self.c().execute(
            "INSERT OR REPLACE INTO pads (id, group_id, readonly_id, created, last_edited) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, group_id, readonly_id, created, last_edited],
        )?;
        Ok(())
    }
    pub fn import_author(&self, id: &str, mapper: Option<&str>, name: &str, color: Option<i32>) -> Result<()> {
        let _ = color; // Farbe wird aus der Kennung abgeleitet (stabil); Etherpad-Farbindex bleibt im Export
        self.c().execute("INSERT OR REPLACE INTO authors (id, mapper, name, created) VALUES (?1, ?2, ?3, ?4)", params![id, mapper, name, ids::now_ms()])?;
        Ok(())
    }
    /// Setzt das Update-Log eines Pads auf genau einen Eintrag (den Gesamtzustand).
    pub fn import_initial_update(&self, pad_id: &str, state: &[u8]) -> Result<i64> {
        let c = self.c();
        c.execute("DELETE FROM updates WHERE pad_id=?1", params![pad_id])?;
        c.execute("DELETE FROM snapshots WHERE pad_id=?1", params![pad_id])?;
        c.execute("INSERT INTO updates (pad_id, seq, ts, author_id, bytes) VALUES (?1, 1, ?2, NULL, ?3)", params![pad_id, ids::now_ms(), state])?;
        Ok(1)
    }
    pub fn import_snapshot(&self, pad_id: &str, ts: i64, label: &str, state: &[u8], html: &str) -> Result<()> {
        self.c().execute(
            "INSERT OR REPLACE INTO snapshots (pad_id, ts, seq, label, state, html) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
            params![pad_id, ts, label, state, html],
        )?;
        Ok(())
    }
}

impl From<rusqlite::Error> for CreatePadError {
    fn from(e: rusqlite::Error) -> Self { CreatePadError::Other(anyhow!(e)) }
}

/// Profilbild aus Moodle: `user/pix.php/<id>/f1.jpg` liefert das Bild oder Moodles Platzhalter.
fn avatar_url(moodle_url: Option<String>, moodle_user: Option<String>) -> Option<String> {
    let (u, id) = (moodle_url?, moodle_user?);
    id.chars().all(|c| c.is_ascii_digit()).then(|| format!("{u}/user/pix.php/{id}/f1.jpg"))
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

const MESS_TABELLEN: [&str; 4] = ["messung_marke", "messung_runde", "messung_probe", "messung_geraet"];
