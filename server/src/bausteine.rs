//! Bausteine im Pad, die mehr als Text sind: Bewertungen (Daumen hoch/runter) und Inhaltsverzeichnis.
//! Bewertungen: Der Knoten "rating" trägt nur seine Kennung, die Stimmen liegen in der Y.Map "ratings"
//! unter "<kennung>:<autor>" mit 1 oder -1 (so wie web/src/rating.js sie schreibt).
use yrs::types::text::YChange;
use yrs::{Any, Map, Text, Number, Out, ReadTxn, TransactionMut, WriteTxn, Xml, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut};

pub const RATINGS: &str = "ratings";

fn vote_value(v: &Out) -> Option<i8> {
    let n = match v { Out::Any(Any::Number(Number::Int(n))) => *n as f64, Out::Any(Any::Number(Number::Float(f))) => *f, _ => return None };
    if n == 1.0 { Some(1) } else if n == -1.0 { Some(-1) } else { None }
}

/// Stimmen einer Bewertung als (Autor, ±1), sortiert.
pub fn votes<T: ReadTxn>(txn: &T, id: &str) -> Vec<(String, i8)> {
    let Some(m) = txn.get_map(RATINGS) else { return Vec::new() };
    let prefix = format!("{id}:");
    let mut out: Vec<(String, i8)> = m.iter(txn).filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_string(), vote_value(&v)?))).collect();
    out.sort();
    out
}

/// (Daumen hoch, Daumen runter)
pub fn tally(votes: &[(String, i8)]) -> (usize, usize) {
    (votes.iter().filter(|v| v.1 == 1).count(), votes.iter().filter(|v| v.1 == -1).count())
}

pub fn rating_id<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> String {
    match el.get_attribute(txn, "id") { Some(Out::Any(Any::String(s))) => s.to_string(), _ => String::new() }
}

/// Stimmen für den HTML-Export und -Import: "autor:1,autor:-1" (Kennungen enthalten weder ':' noch ',').
pub fn votes_attr(votes: &[(String, i8)]) -> String {
    votes.iter().map(|(a, d)| format!("{a}:{d}")).collect::<Vec<_>>().join(",")
}
pub fn parse_votes_attr(s: &str) -> Vec<(String, i8)> {
    s.split(',').filter_map(|p| {
        let (a, d) = p.trim().rsplit_once(':')?;
        let ok = !a.is_empty() && a.len() <= 64 && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        let d: i8 = d.parse().ok()?;
        (ok && (d == 1 || d == -1)).then(|| (a.to_string(), d))
    }).take(10_000).collect()
}

pub fn clean_id(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| s.to_string())
}

pub fn write_votes(txn: &mut TransactionMut, id: &str, votes: &[(String, i8)]) {
    let m = txn.get_or_insert_map(RATINGS);
    for (a, d) in votes { m.insert(txn, format!("{id}:{a}"), Any::from(*d as f64)); }
}

/// Alle Stimmen verwerfen (beim Ersetzen des ganzen Inhalts).
pub fn clear_votes(txn: &mut TransactionMut) {
    if txn.get_map(RATINGS).is_some() { txn.get_or_insert_map(RATINGS).clear(txn); }
}

/// Überschriften (Ebene, Text) in Dokumentreihenfolge, ohne solche in Tabellen – wie web/src/toc.js.
pub fn headings<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> Vec<(u8, String)> {
    fn walk<T: ReadTxn>(txn: &T, el: &XmlElementRef, out: &mut Vec<(u8, String)>, depth: usize) {
        if depth > 48 { return; }
        match el.tag().as_ref() {
            "heading" => {
                let level = match el.get_attribute(txn, "level") { Some(Out::Any(Any::Number(Number::Int(n)))) => n, Some(Out::Any(Any::Number(Number::Float(f)))) => f as i64, _ => 1 }.clamp(1, 6) as u8;
                let text = plain_text(txn, el);
                if !text.trim().is_empty() { out.push((level, text.trim().to_string())); }
            }
            "table" => {}
            _ => for c in el.children(txn) { if let XmlOut::Element(e) = c { walk(txn, &e, out, depth + 1); } },
        }
    }
    let mut out = Vec::new();
    for c in frag.children(txn) { if let XmlOut::Element(e) = c { walk(txn, &e, &mut out, 0); } }
    out
}

/// Reiner Text eines Elements (ohne Marks), auch aus verschachtelten Inline-Knoten.
pub fn plain_text<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> String {
    let mut out = String::new();
    for c in el.children(txn) {
        match c {
            XmlOut::Text(t) => for d in t.diff(txn, YChange::identity) { if let Out::Any(Any::String(s)) = d.insert { out.push_str(&s); } },
            XmlOut::Element(e) => out.push_str(&plain_text(txn, &e)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stimmen_attribut_hin_und_zurueck() {
        let v = vec![("a1".to_string(), 1), ("b-2".to_string(), -1)];
        assert_eq!(parse_votes_attr(&votes_attr(&v)), v);
        assert!(parse_votes_attr("x:2,<y>:1,:1").is_empty());
        assert_eq!(tally(&v), (1, 1));
    }
}
