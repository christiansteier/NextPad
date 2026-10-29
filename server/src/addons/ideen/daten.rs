//! Tabellen und Zugriffe der Ideensammlung.
use crate::store::{Store, Zusatz};
use crate::ids;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

pub const ZUSATZ: Zusatz = Zusatz { schema: SCHEMA, ergaenzen, tabellen: &["idea_votes", "ideas", "idea_prompts"] };

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ideas (
  pad_id TEXT NOT NULL, id INTEGER NOT NULL, author_id TEXT NOT NULL, text TEXT NOT NULL, created INTEGER NOT NULL,
  hidden INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (pad_id, id));
CREATE TABLE IF NOT EXISTS idea_votes (pad_id TEXT NOT NULL, idea_id INTEGER NOT NULL, author_id TEXT NOT NULL, PRIMARY KEY (pad_id, idea_id, author_id));
CREATE TABLE IF NOT EXISTS idea_prompts (pad_id TEXT PRIMARY KEY, prompt TEXT NOT NULL);
"#;

fn ergaenzen(_conn: &Connection) -> rusqlite::Result<()> { Ok(()) }

#[derive(Clone, Debug, serde::Serialize)]
pub struct Idea {
    pub id: i64,
    pub text: String,
    pub created: i64,
    pub hidden: bool,
    pub done: bool,
    pub votes: i64,
}

impl Store {
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

    pub fn idea_prompt(&self, pad_id: &str) -> Result<String> {
        Ok(self.c().query_row("SELECT prompt FROM idea_prompts WHERE pad_id=?1", params![pad_id], |r| r.get(0)).optional()?.unwrap_or_default())
    }
    pub fn set_idea_prompt(&self, pad_id: &str, prompt: &str) -> Result<()> {
        self.c().execute("INSERT INTO idea_prompts (pad_id, prompt) VALUES (?1, ?2) ON CONFLICT(pad_id) DO UPDATE SET prompt=excluded.prompt", params![pad_id, prompt])?;
        Ok(())
    }
}
