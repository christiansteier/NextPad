//! Live-Kanal je Pad für Addons: Server-Sent Events, offene Seiten holen nach jeder Meldung den Stand neu.
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::broadcast;

/// Je Pad ein Kanal.
#[derive(Default)]
pub struct Hub(Mutex<HashMap<String, broadcast::Sender<()>>>);

impl Hub {
    pub fn subscribe(&self, pad: &str) -> broadcast::Receiver<()> {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, tx| tx.receiver_count() > 0);
        m.entry(pad.to_string()).or_insert_with(|| broadcast::channel(16).0).subscribe()
    }
    pub fn notify(&self, pad: &str) {
        if let Some(tx) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(pad) { let _ = tx.send(()); }
    }
}
