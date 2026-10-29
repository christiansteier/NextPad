//! Geladene Dokumente: je Pad ein yrs-Dokument mit Awareness, Broadcast an Verbindungen,
//! Persistenz jedes Updates im Log, Snapshots nur bei Änderungen, Entladen im Leerlauf.
use crate::sperre::Sperre;
use crate::{ids, render, store::Store};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use yrs::sync::{Awareness, Message, SyncMessage};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, OffsetKind, Options, ReadTxn, StateVector, Transact, TransactionMut, Update, UpdateEvent};

/// Punkt der Zeitleiste: Vollstand (Zeitstempel) oder Stand nach einem Schritt (Nummer im Update-Log).
#[derive(Clone, Copy, Debug)]
pub enum Punkt { Stand(i64), Schritt(i64) }

/// Kapazität des Broadcast-Kanals je Pad; wer weiter zurückliegt, wird getrennt und synchronisiert neu.
const CHANNEL: usize = 1024;

pub struct Room {
    pub pad_id: String,
    pub awareness: AsyncMutex<Awareness>,
    pub tx: broadcast::Sender<Arc<Vec<u8>>>,
    clients: AtomicUsize,
    /// Zeit des letzten Updates, des letzten Snapshots und des letzten Zugriffs (ms)
    pub last_update: Arc<AtomicI64>,
    pub last_snapshot: AtomicI64,
    pub last_access: AtomicI64,
    /// Sequenznummer des letzten Updates, das in diesem Dokument enthalten ist
    pub last_seq: Arc<AtomicI64>,
    /// Pad wurde gelöscht: nichts mehr speichern
    pub deleted: Arc<AtomicBool>,
}

impl Room {
    pub fn clients(&self) -> usize { self.clients.load(Ordering::SeqCst) }
    fn touch(&self) { self.last_access.store(ids::now_ms(), Ordering::Relaxed); }
    fn dirty(&self) -> bool { self.last_update.load(Ordering::Relaxed) > self.last_snapshot.load(Ordering::Relaxed) }
}

/// Hält eine Verbindung an einem Raum; zählt beim Verwerfen herunter.
pub struct ClientGuard { pub room: Arc<Room> }
impl Drop for ClientGuard {
    fn drop(&mut self) { self.room.clients.fetch_sub(1, Ordering::SeqCst); self.room.touch(); }
}

pub struct Rooms {
    store: Arc<Store>,
    rooms: Mutex<HashMap<String, Arc<Room>>>,
}

impl Rooms {
    pub fn new(store: Arc<Store>) -> Self { Rooms { store, rooms: Mutex::new(HashMap::new()) } }

    /// Verbundene Clients eines Pads (0, wenn nicht geladen).
    pub fn clients_of(&self, pad_id: &str) -> usize { self.rooms.sperren().get(pad_id).map(|r| r.clients()).unwrap_or(0) }

    pub fn loaded(&self) -> usize { self.rooms.sperren().len() }

    /// Lädt ein Pad (Snapshot plus Update-Log) oder liefert das bereits geladene.
    pub fn get(&self, pad_id: &str) -> Result<Arc<Room>> {
        if let Some(r) = self.rooms.sperren().get(pad_id) { r.touch(); return Ok(r.clone()); }
        let room = self.load(pad_id)?;
        let mut map = self.rooms.sperren();
        let r = map.entry(pad_id.to_string()).or_insert(room).clone();
        r.touch();
        Ok(r)
    }

    /// Wie `get`, zählt die Verbindung aber unter derselben Sperre (kein Entladen dazwischen).
    pub fn join(&self, pad_id: &str) -> Result<ClientGuard> {
        {
            let map = self.rooms.sperren();
            if let Some(r) = map.get(pad_id) { r.clients.fetch_add(1, Ordering::SeqCst); r.touch(); return Ok(ClientGuard { room: r.clone() }); }
        }
        let room = self.load(pad_id)?;
        let mut map = self.rooms.sperren();
        let r = map.entry(pad_id.to_string()).or_insert(room).clone();
        r.clients.fetch_add(1, Ordering::SeqCst);
        r.touch();
        Ok(ClientGuard { room: r })
    }

    fn load(&self, pad_id: &str) -> Result<Arc<Room>> {
        let doc = Doc::with_options(Options { skip_gc: true, offset_kind: OffsetKind::Utf16, ..Default::default() });
        let _ = doc.get_or_insert_xml_fragment(render::FRAGMENT);
        // erst alles Wartende schreiben lassen (Pad kann gerade entladen worden sein); sonst fehlten Änderungen
        if !self.store.flush() { return Err(anyhow::anyhow!("Speichern hängt, Pad wird nicht geladen")); }
        let mut seq: i64 = 0;
        {
            let mut txn = doc.transact_mut();
            if let Some((s, state)) = self.store.latest_snapshot(pad_id)? {
                if let Ok(u) = Update::decode_v1(&state) { let _ = txn.apply_update(u); seq = s; }
            }
            for (s, bytes) in self.store.updates_since(pad_id, seq)? {
                if let Ok(u) = Update::decode_v1(&bytes) { let _ = txn.apply_update(u); }
                seq = s;
            }
        }
        let last_seq = Arc::new(AtomicI64::new(seq));
        let deleted = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = broadcast::channel::<Arc<Vec<u8>>>(CHANNEL);
        let now = ids::now_ms();
        let last_update = Arc::new(AtomicI64::new(0));
        {
            let (tx_doc, store, pid, ls, del, lu) = (tx.clone(), self.store.clone(), pad_id.to_string(), last_seq.clone(), deleted.clone(), last_update.clone());
            let _ = doc.observe_update_v1("nextpad", move |txn: &TransactionMut, ev: &UpdateEvent| {
                if del.load(Ordering::SeqCst) { return; }
                let bytes = ev.update.clone();
                let author: Option<String> = txn.origin().and_then(|o| std::str::from_utf8(o.as_ref()).ok()).filter(|a| a.starts_with("a.")).map(str::to_string);
                // Nummer im Raum vergeben, Schreiben übernimmt der Schreib-Thread (kein SQLite unter der Raumsperre)
                let seq = ls.fetch_add(1, Ordering::SeqCst) + 1;
                store.queue_update(&pid, seq, &bytes, author.as_deref());
                lu.store(ids::now_ms(), Ordering::Relaxed);
                let _ = tx_doc.send(Arc::new(Message::Sync(SyncMessage::Update(bytes)).encode_v1()));
            });
        }
        let mut awareness = Awareness::new(doc);
        let tx_aw = tx.clone();
        awareness.on_update("nextpad", move |aw: &Awareness, ev: &yrs::sync::awareness::Event, _origin| {
            let changed: Vec<_> = ev.added().iter().chain(ev.updated()).chain(ev.removed()).copied().collect();
            if let Ok(u) = aw.update_with_clients(changed) { let _ = tx_aw.send(Arc::new(Message::Awareness(u).encode_v1())); }
        });
        Ok(Arc::new(Room {
            pad_id: pad_id.to_string(), awareness: AsyncMutex::new(awareness), tx, clients: AtomicUsize::new(0),
            last_update, last_snapshot: AtomicI64::new(now), last_access: AtomicI64::new(now),
            last_seq, deleted,
        }))
    }

    /// Snapshot schreiben (Zustand und HTML), nur wenn das Pad noch existiert.
    pub async fn snapshot(&self, room: &Room, label: &str) -> Result<()> {
        if room.deleted.load(Ordering::SeqCst) { return Ok(()); }
        let aw = room.awareness.lock().await;
        let doc = aw.doc();
        let frag = doc.get_or_insert_xml_fragment(render::FRAGMENT);
        let (state, html) = { let txn = doc.transact(); (txn.encode_state_as_update_v1(&StateVector::default()), render::to_html(&txn, &frag)) };
        let seq = room.last_seq.load(Ordering::SeqCst);
        if !self.store.save_snapshot(&room.pad_id, seq, label, &state, &html)? { room.deleted.store(true, Ordering::SeqCst); }
        room.last_snapshot.store(ids::now_ms(), Ordering::Relaxed);
        Ok(())
    }

    /// Raum entladen, wenn niemand verbunden ist und er noch der aktuelle Eintrag ist.
    fn unload_if_idle(&self, room: &Arc<Room>) {
        let mut map = self.rooms.sperren();
        if room.clients() == 0 && map.get(&room.pad_id).is_some_and(|r| Arc::ptr_eq(r, room)) { map.remove(&room.pad_id); }
    }

    /// Letzte Verbindung getrennt: Snapshot nur bei Änderungen, dann entladen.
    pub async fn release(&self, room: &Arc<Room>) {
        if room.clients() > 0 { return; }
        if room.dirty() { if let Err(e) = self.snapshot(room, "auto").await { tracing::warn!(pad = %room.pad_id, "Snapshot: {e}"); } }
        self.unload_if_idle(room);
    }

    /// Wartung im Minutentakt: Snapshot nach Ruhephase oder Höchstabstand, Entladen unbenutzter Räume.
    pub async fn maintain(&self, idle_ms: i64, max_ms: i64, unload_ms: i64) {
        let rooms: Vec<Arc<Room>> = self.rooms.sperren().values().cloned().collect();
        let now = ids::now_ms();
        for room in rooms {
            let lu = room.last_update.load(Ordering::Relaxed);
            let ls = room.last_snapshot.load(Ordering::Relaxed);
            if lu > ls && (now - lu >= idle_ms || now - ls >= max_ms) {
                let label = if room.clients() == 0 { "auto" } else { "Ruhephase" };
                if let Err(e) = self.snapshot(&room, label).await { tracing::warn!(pad = %room.pad_id, "Snapshot: {e}"); }
            }
            if room.clients() == 0 && !room.dirty() && now - room.last_access.load(Ordering::Relaxed) >= unload_ms { self.unload_if_idle(&room); }
        }
    }

    /// Snapshot aller geladenen Pads mit Änderungen (beim Beenden).
    pub async fn snapshot_all(&self, label: &str) {
        let rooms: Vec<Arc<Room>> = self.rooms.sperren().values().cloned().collect();
        for room in rooms { if room.dirty() { let _ = self.snapshot(&room, label).await; } }
    }

    async fn read<T>(&self, pad_id: &str, f: impl FnOnce(&yrs::Transaction, &yrs::XmlFragmentRef) -> T) -> Result<T> {
        let room = self.get(pad_id)?;
        let aw = room.awareness.lock().await;
        let doc = aw.doc();
        let frag = doc.get_or_insert_xml_fragment(render::FRAGMENT);
        let txn = doc.transact();
        Ok(f(&txn, &frag))
    }
    pub async fn html(&self, pad_id: &str) -> Result<String> { self.read(pad_id, |t, f| render::to_html(t, f)).await }
    pub async fn text(&self, pad_id: &str) -> Result<String> { self.read(pad_id, |t, f| render::to_text(t, f)).await }
    pub async fn markdown(&self, pad_id: &str) -> Result<String> { self.read(pad_id, |t, f| render::to_markdown(t, f)).await }
    pub async fn typst(&self, pad_id: &str) -> Result<String> { self.read(pad_id, |t, f| render::to_typst(t, f)).await }
    /// Exporte in der Sprache der Anfrage: `toc` ist der Titel des Inhaltsverzeichnisses.
    pub async fn html_mit(&self, pad_id: &str, toc: &'static str) -> Result<String> { self.read(pad_id, |t, f| render::to_html_mit(t, f, toc)).await }
    pub async fn text_mit(&self, pad_id: &str, toc: &'static str) -> Result<String> { self.read(pad_id, |t, f| render::to_text_mit(t, f, toc)).await }
    pub async fn markdown_mit(&self, pad_id: &str, toc: &'static str) -> Result<String> { self.read(pad_id, |t, f| render::to_markdown_mit(t, f, toc)).await }
    pub async fn typst_mit(&self, pad_id: &str, toc: &'static str) -> Result<String> { self.read(pad_id, |t, f| render::to_typst_mit(t, f, toc)).await }

    async fn write(&self, pad_id: &str, label: &str, f: impl FnOnce(&mut TransactionMut, &yrs::XmlFragmentRef)) -> Result<()> {
        let room = self.get(pad_id)?;
        {
            let aw = room.awareness.lock().await;
            let doc = aw.doc();
            let frag = doc.get_or_insert_xml_fragment(render::FRAGMENT);
            let mut txn = doc.transact_mut();
            f(&mut txn, &frag);
        }
        self.snapshot(&room, label).await
    }
    pub async fn set_text(&self, pad_id: &str, text: &str) -> Result<()> { self.write(pad_id, "setText", |t, f| render::set_text(t, f, text)).await }
    pub async fn set_html(&self, pad_id: &str, html: &str) -> Result<()> { self.write(pad_id, "setHTML", |t, f| render::set_html(t, f, html)).await }

    /// Früheren Stand aus der Zeitleiste zurückholen: als neue Änderung obendrauf, der jetzige Stand bleibt im Verlauf.
    /// None, wenn es den Punkt nicht (mehr) gibt.
    pub async fn restore(&self, pad_id: &str, punkt: Punkt) -> Result<Option<()>> {
        let Some(html) = self.html_at(pad_id, punkt)? else { return Ok(None) };
        self.write(pad_id, "Wiederhergestellt", |t, f| render::set_html(t, f, &html)).await?;
        Ok(Some(()))
    }

    /// Stand an einem Punkt der Zeitleiste als HTML, nachgebaut wie im Browser: Vollstand, danach die Schritte bis dahin.
    fn html_at(&self, pad_id: &str, punkt: Punkt) -> Result<Option<String>> {
        if !self.store.flush() { return Err(anyhow::anyhow!("Speichern hängt")); }
        let snaps = self.store.snapshots_all(pad_id)?;
        let doc = Doc::with_options(Options { skip_gc: true, offset_kind: OffsetKind::Utf16, ..Default::default() });
        let frag = doc.get_or_insert_xml_fragment(render::FRAGMENT);
        {
            let mut txn = doc.transact_mut();
            match punkt {
                Punkt::Stand(ts) => {
                    let Some((_, _, _, state)) = snaps.iter().find(|s| s.0 == ts) else { return Ok(None) };
                    txn.apply_update(Update::decode_v1(state)?)?;
                }
                Punkt::Schritt(seq) => {
                    let ups = self.store.updates_all(pad_id)?;
                    if !ups.iter().any(|u| u.0 == seq) { return Ok(None); }
                    let base = snaps.iter().filter(|s| s.1 <= seq).max_by_key(|s| s.1);
                    let von = match base { Some((_, s, _, state)) => { txn.apply_update(Update::decode_v1(state)?)?; *s } None => 0 };
                    for (_, _, _, bytes) in ups.iter().filter(|u| u.0 > von && u.0 <= seq) { txn.apply_update(Update::decode_v1(bytes)?)?; }
                }
            }
        }
        let txn = doc.transact();
        Ok(Some(render::to_html(&txn, &frag)))
    }

    /// Pad gelöscht: Raum sperren, aus dem Speicher nehmen, Verbindungen schließen.
    pub fn evict(&self, pad_id: &str) {
        if let Some(room) = self.rooms.sperren().remove(pad_id) {
            room.deleted.store(true, Ordering::SeqCst);
            let _ = room.tx.send(Arc::new(Vec::new())); // leere Nachricht = Verbindung schließen
        }
    }
}

