//! Kennungen im Etherpad-Format: Präfix plus 16 zufällige alphanumerische Zeichen.
use rand::{distr::Alphanumeric, Rng};

pub fn random16() -> String {
    rand::rng().sample_iter(&Alphanumeric).take(16).map(char::from).collect()
}

pub fn group_id() -> String { format!("g.{}", random16()) }
pub fn author_id() -> String { format!("a.{}", random16()) }
pub fn session_id() -> String { format!("s.{}", random16()) }
pub fn readonly_id() -> String { format!("r.{}", random16()) }

/// Gruppenkennung aus einer Pad-Kennung (`g.xxx$name` → `g.xxx`), wie das Moodle-Plugin es tut.
pub fn group_of_pad(pad_id: &str) -> Option<&str> {
    if pad_id.starts_with("g.") {
        pad_id.split('$').next()
    } else {
        None
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Farbindex eines Autors aus seiner Kennung (stabil, 0..PALETTE).
pub fn color_index(author_id: &str, palette_len: usize) -> usize {
    let mut h: u64 = 1469598103934665603;
    for b in author_id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    (h % palette_len as u64) as usize
}
