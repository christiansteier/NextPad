//! Persistenz: SQLite mit Metadaten, Update-Log und Snapshots.
use crate::sperre::Sperre;
use crate::ids;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::{mpsc, Mutex}, time::{Duration, Instant}};

pub struct Store {
    conn: Mutex<Connection>,
    /// Schreib-Thread für Yjs-Updates und Wartung (eigene Verbindung, Transaktionen gebündelt)
    writer: mpsc::SyncSender<WriteOp>,
    /// Tabellen der Addons mit Spalte `pad_id`; sie gehen beim Löschen eines Pads mit
    zusatz: Vec<&'static str>,
}

/// Eigene Tabellen eines Addons: Schema, Ergänzungen älterer Datenbanken und die Tabellen,
/// die beim Löschen eines Pads mitgehen. Angelegt werden sie immer, auch wenn das Addon ausgeschaltet ist.
pub struct Zusatz {
    pub schema: &'static str,
    pub ergaenzen: fn(&Connection) -> rusqlite::Result<()>,
    pub tabellen: &'static [&'static str],
}

/// Hat die Tabelle die Spalte? Für Schema-Ergänzungen.
pub fn hat_spalte(conn: &Connection, tabelle: &str, spalte: &str) -> rusqlite::Result<bool> {
    conn.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name=?2")?.exists(params![tabelle, spalte])
}

/// Aufträge an den Schreib-Thread. Reihenfolge bleibt erhalten.
/// Wartungsauftrag für den Schreib-Thread
type Job = Box<dyn FnOnce(&Connection) -> rusqlite::Result<()> + Send>;
/// Update-Log: Nummer, Zeit (ms), Autor, Yjs-Update
pub type UpdateRow = (i64, i64, Option<String>, Vec<u8>);
/// Vollstand: Zeit (ms), letzte enthaltene Nummer, Bezeichnung, Yjs-Zustand
pub type SnapshotRow = (i64, i64, String, Vec<u8>);

pub enum WriteOp {
    Append { pad: String, seq: i64, ts: i64, author: Option<String>, bytes: Vec<u8> },
    Exec(Job),
    Flush(mpsc::Sender<bool>),
}

/// Ein Yjs-Update, wie es in die Tabelle `updates` geht; bleibt erhalten, bis es geschrieben ist.
struct Pending { pad: String, seq: i64, ts: i64, author: Option<String>, bytes: Vec<u8> }

fn write_updates(conn: &Connection, ups: &[Pending]) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let mut touched: std::collections::HashMap<&str, i64> = Default::default();
    {
        let mut ins = tx.prepare_cached(
            "INSERT OR IGNORE INTO updates (pad_id, seq, ts, author_id, bytes) SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM pads WHERE id=?1)")?;
        for u in ups { ins.execute(params![u.pad, u.seq, u.ts, u.author, u.bytes])?; touched.insert(&u.pad, u.ts); }
    }
    for (pad, ts) in touched { tx.execute("UPDATE pads SET last_edited=?2 WHERE id=?1 AND last_edited < ?2", params![pad, ts])?; }
    tx.commit()
}

/// Bündelt Updates: wartet nach dem ersten Auftrag höchstens 5 ms oder bis 512 Aufträge, schreibt alles in einer Transaktion.
/// Scheitert die Transaktion, versucht der Thread es mit wachsender Pause erneut, bis sie gelingt: die Updates sind schon
/// bei den Verbundenen angekommen und dürfen nicht verloren gehen. Solange staut sich die Warteschlange (Gegendruck).
/// Wartungsaufträge laufen danach je in eigener Transaktion; scheitert einer, kommt er beim nächsten Lauf wieder.
fn writer_loop(conn: Connection, rx: mpsc::Receiver<WriteOp>) {
    writer_loop_with(conn, rx, Duration::from_millis(50), Duration::from_secs(5))
}

fn writer_loop_with(conn: Connection, rx: mpsc::Receiver<WriteOp>, first_pause: Duration, max_pause: Duration) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        let deadline = Instant::now() + Duration::from_millis(5);
        while batch.len() < 512 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() { break; }
            match rx.recv_timeout(left) { Ok(op) => batch.push(op), Err(_) => break }
        }
        let (mut ups, mut jobs, mut flushes) = (Vec::new(), Vec::new(), Vec::new());
        for op in batch {
            match op {
                WriteOp::Append { pad, seq, ts, author, bytes } => ups.push(Pending { pad, seq, ts, author, bytes }),
                WriteOp::Exec(f) => jobs.push(f),
                WriteOp::Flush(s) => flushes.push(s),
            }
        }
        if !ups.is_empty() {
            let mut pause = first_pause;
            let mut tries = 0u32;
            while let Err(e) = write_updates(&conn, &ups) {
                tries += 1;
                tracing::error!("Schreib-Thread: {} Updates nicht gespeichert (Versuch {tries}), neuer Versuch in {} ms: {e}", ups.len(), pause.as_millis());
                std::thread::sleep(pause);
                pause = (pause * 2).min(max_pause);
            }
            if tries > 0 { tracing::warn!("Schreib-Thread: {} Updates nach {} Fehlversuchen gespeichert", ups.len(), tries); }
        }
        for f in jobs {
            let res = conn.unchecked_transaction().and_then(|tx| { f(&tx)?; tx.commit() });
            if let Err(e) = res { tracing::warn!("Schreib-Thread, Wartung: {e}"); }
        }
        for s in flushes { let _ = s.send(true); }
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
    /// zuletzt gewählte Sprache (`de`, `en`), leer wenn nie gewählt
    pub owner_sprache: Option<String>,
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
CREATE TABLE IF NOT EXISTS lti_links (link_key TEXT PRIMARY KEY, pad_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pad_join (pad_id TEXT PRIMARY KEY, url TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pad_kurs (pad_id TEXT PRIMARY KEY, url TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pad_moderators (pad_id TEXT NOT NULL, author_id TEXT NOT NULL, PRIMARY KEY (pad_id, author_id));
"#;

impl Store {
    pub fn open(path: &Path, zusaetze: &[&Zusatz]) -> Result<Self> {
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
        // Schema-Ergänzung 29.10.2026: zuletzt gewählte Sprache einer über Moodle angemeldeten Person (für die Mail vor dem Löschen)
        let has_sprache: bool = conn.prepare("SELECT 1 FROM pragma_table_info('authors') WHERE name='sprache'")?.exists([])?;
        if !has_sprache { conn.execute_batch("ALTER TABLE authors ADD COLUMN sprache TEXT;")?; }
        for z in zusaetze { conn.execute_batch(z.schema)?; (z.ergaenzen)(&conn)?; }
        let wconn = Connection::open(path)?;
        wconn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        let (tx, rx) = mpsc::sync_channel::<WriteOp>(65_536);
        std::thread::Builder::new().name("sqlite-writer".into()).spawn(move || writer_loop(wconn, rx))?;
        Ok(Store { conn: Mutex::new(conn), writer: tx, zusatz: zusaetze.iter().flat_map(|z| z.tabellen.iter().copied()).collect() })
    }

    pub(crate) fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.sperren()
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
            Self::delete_pad_in(&c, &self.zusatz, p)?;
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
    fn delete_pad_in(c: &Connection, zusatz: &[&str], id: &str) -> Result<bool> {
        c.execute("DELETE FROM updates WHERE pad_id=?1", params![id])?;
        // Tabellen der Addons, auch ausgeschalteter: deren Daten gehen mit dem Pad
        for t in zusatz { c.execute(&format!("DELETE FROM {t} WHERE pad_id=?1"), params![id])?; }
        c.execute("DELETE FROM pad_moderators WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM lti_links WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM pad_join WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM pad_kurs WHERE pad_id=?1", params![id])?;
        c.execute("DELETE FROM snapshots WHERE pad_id=?1", params![id])?;
        Ok(c.execute("DELETE FROM pads WHERE id=?1", params![id])? > 0)
    }
    pub fn delete_pad(&self, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let r = Self::delete_pad_in(&tx, &self.zusatz, id)?;
        tx.commit()?;
        Ok(r)
    }
    // Pad-Typ
    pub fn set_kind(&self, pad_id: &str, kind: &str) -> Result<()> {
        self.c().execute("UPDATE pads SET kind=?2 WHERE id=?1", params![pad_id, kind])?;
        Ok(())
    }
    /// Pad-Typ: None = Text-Pad, sonst die Art eines Addons.
    pub fn kind(&self, pad_id: &str) -> Option<String> {
        self.c().query_row("SELECT kind FROM pads WHERE id=?1", params![pad_id], |r| r.get::<_, Option<String>>(0)).ok().flatten()
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
        // Text-Pads haben keine eigene Art (NULL), wie über die Etherpad-API angelegte
        let kind = (kind != "pad").then_some(kind);
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
        let mut st = c.prepare("SELECT p.id, p.created, p.last_edited, p.last_seen, p.keep_until, p.notice_sent, p.delete_after, p.owner_id, a.email, a.name, a.sprache
            FROM pads p LEFT JOIN authors a ON a.id=p.owner_id WHERE p.group_id IS NULL")?;
        let rows = st.query_map([], |r| Ok(RetentionRow {
            id: r.get(0)?, created: r.get(1)?, last_edited: r.get(2)?, last_seen: r.get(3)?, keep_until: r.get(4)?,
            notice_sent: r.get(5)?, delete_after: r.get(6)?, owner_id: r.get(7)?, email: r.get(8)?, owner_name: r.get(9)?, owner_sprache: r.get(10)?,
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
    pub fn set_author_sprache(&self, author_id: &str, sprache: &str) -> Result<()> {
        self.c().execute("UPDATE authors SET sprache=?2 WHERE id=?1", params![author_id, sprache])?;
        Ok(())
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
    /// false, wenn das nach 30 s nicht geschafft ist (Datenbank scheitert dauerhaft, der Schreib-Thread versucht es weiter).
    pub fn flush(&self) -> bool {
        let (tx, rx) = mpsc::channel();
        if self.writer.send(WriteOp::Flush(tx)).is_err() { tracing::error!("Schreib-Thread beendet"); return false; }
        let ok = rx.recv_timeout(Duration::from_secs(30)).unwrap_or(false);
        if !ok { tracing::error!("Schreib-Thread: Warteschlange nach 30 s nicht geschrieben"); }
        ok
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
    pub fn updates_all(&self, pad_id: &str) -> Result<Vec<UpdateRow>> {
        Ok(self
            .c()
            .prepare("SELECT seq, ts, author_id, bytes FROM updates WHERE pad_id=?1 ORDER BY seq")?
            .query_map(params![pad_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<_, _>>()?)
    }
    /// Alle Snapshots eines Pads (ts, seq, label, state).
    pub fn snapshots_all(&self, pad_id: &str) -> Result<Vec<SnapshotRow>> {
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

impl Store {
    /// Adresse, über die man in eine Moodle-Aktivität hineinkommt (für den QR-Code am Beamer).
    pub fn set_join_url(&self, pad_id: &str, url: &str) -> Result<()> {
        self.c().execute("INSERT INTO pad_join (pad_id, url) VALUES (?1, ?2) ON CONFLICT(pad_id) DO UPDATE SET url=?2", params![pad_id, url])?;
        Ok(())
    }
    pub fn join_url(&self, pad_id: &str) -> Option<String> {
        self.c().query_row("SELECT url FROM pad_join WHERE pad_id=?1", params![pad_id], |r| r.get(0)).optional().ok().flatten()
    }
    /// Kursseite in Moodle, aus der die Aktivität gestartet wurde (für „Zurück zum Kurs“).
    pub fn set_course_url(&self, pad_id: &str, url: &str) -> Result<()> {
        self.c().execute("INSERT INTO pad_kurs (pad_id, url) VALUES (?1, ?2) ON CONFLICT(pad_id) DO UPDATE SET url=?2", params![pad_id, url])?;
        Ok(())
    }
    pub fn course_url(&self, pad_id: &str) -> Option<String> {
        self.c().query_row("SELECT url FROM pad_kurs WHERE pad_id=?1", params![pad_id], |r| r.get(0)).optional().ok().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tabellen eines Addons legt der Speicher an und räumt sie mit dem Pad weg.
    #[test]
    fn zusatz_tabellen_gehen_mit_dem_pad() {
        let dir = std::env::temp_dir().join(format!("nextpad-test-z-{}-{}", std::process::id(), ids::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        fn nichts(_: &Connection) -> rusqlite::Result<()> { Ok(()) }
        let z = Zusatz { schema: "CREATE TABLE IF NOT EXISTS probe (pad_id TEXT NOT NULL, x INTEGER);", ergaenzen: nichts, tabellen: &["probe"] };
        let store = Store::open(&dir.join("t.sqlite"), &[&z]).unwrap();
        for p in ["p1", "p2"] { store.create_pad(p, None).unwrap(); store.c().execute("INSERT INTO probe (pad_id, x) VALUES (?1, 1)", params![p]).unwrap(); }
        assert!(store.delete_pad("p1").unwrap());
        let n = |p: &str| store.c().query_row("SELECT COUNT(*) FROM probe WHERE pad_id=?1", params![p], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!((n("p1"), n("p2")), (0, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ältere Datenbank ohne Spalte `authors.sprache`: wird ergänzt, Bestand bleibt, die Wahl landet in der Aufbewahrung.
    #[test]
    fn sprache_der_person_fuer_die_mail() {
        let dir = std::env::temp_dir().join(format!("nextpad-test-s-{}-{}", std::process::id(), ids::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sqlite");
        let store = Store::open(&path, &[]).unwrap();
        let a = store.create_author("Kim", Some("moodle:1")).unwrap();
        store.create_pad("p1", None).unwrap(); store.set_owner("p1", &a.id).unwrap();
        drop(store);
        Connection::open(&path).unwrap().execute_batch("ALTER TABLE authors DROP COLUMN sprache;").unwrap();
        let store = Store::open(&path, &[]).unwrap();
        assert_eq!(store.retention_row("p1").unwrap().unwrap().owner_sprache, None);
        store.set_author_sprache(&a.id, "en").unwrap();
        let r = store.retention_row("p1").unwrap().unwrap();
        assert_eq!((r.owner_name.as_deref(), r.owner_sprache.as_deref()), (Some("Kim"), Some("en")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Gesperrte Datenbank: der Stapel wird nicht verworfen, sondern geschrieben, sobald die Sperre weg ist.
    #[test]
    fn schreibfehler_werden_wiederholt() {
        let dir = std::env::temp_dir().join(format!("nextpad-test-{}-{}", std::process::id(), ids::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sqlite");
        let store = Store::open(&path, &[]).unwrap();
        store.create_pad("p1", None).unwrap();
        // eigener Schreib-Thread ohne Wartezeit bei Sperre, damit der Fehler sofort kommt
        let wconn = Connection::open(&path).unwrap();
        wconn.execute_batch("PRAGMA busy_timeout=0;").unwrap();
        let (tx, rx) = mpsc::sync_channel::<WriteOp>(16);
        std::thread::spawn(move || writer_loop_with(wconn, rx, Duration::from_millis(20), Duration::from_millis(100)));
        let sperre = Connection::open(&path).unwrap();
        sperre.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        tx.send(WriteOp::Append { pad: "p1".into(), seq: 1, ts: 1, author: None, bytes: vec![1, 2, 3] }).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        sperre.execute_batch("COMMIT;").unwrap();
        let (ftx, frx) = mpsc::channel();
        tx.send(WriteOp::Flush(ftx)).unwrap();
        assert!(frx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert_eq!(store.updates_since("p1", 0).unwrap(), vec![(1, vec![1, 2, 3])]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
