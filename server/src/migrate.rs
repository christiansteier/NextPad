//! Import eines Etherpad-Exports (Verzeichnis aus `ep-export.py`): Autoren, Gruppen, Pads,
//! aktueller Stand aus `pad.json`, Tagesstände aus `revisionen.jsonl` per Changeset-Replay.
//! Nur lesend auf dem Export, schreibt ausschließlich in das NextPad-Datenverzeichnis.
use crate::render::{self, Block, Marks};
use crate::store::Store;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use yrs::{Doc, OffsetKind, Options, ReadTxn, StateVector, Transact};

#[derive(Deserialize)]
struct Manifest {
    pads_liste: Vec<ManifestPad>,
}
#[derive(Deserialize, Clone)]
struct ManifestPad {
    #[serde(rename = "padID")]
    pad_id: String,
    #[serde(rename = "groupID")]
    group_id: Option<String>,
    #[serde(rename = "readOnlyID")]
    readonly_id: Option<String>,
    #[serde(rename = "lastEdited_ms")]
    last_edited_ms: Option<i64>,
    created: Option<String>,
}
#[derive(Deserialize)]
struct PadJson {
    atext: AText,
    pool: Pool,
    head: Option<i64>,
}
#[derive(Deserialize, Clone)]
struct AText {
    text: String,
    attribs: String,
}
#[derive(Deserialize, Default, Clone)]
struct Pool {
    #[serde(rename = "numToAttrib")]
    num_to_attrib: HashMap<String, (String, String)>,
}
#[derive(Deserialize)]
struct RevLine {
    rev: i64,
    value: RevValue,
}
#[derive(Deserialize)]
struct RevValue {
    changeset: String,
    meta: RevMeta,
}
#[derive(Deserialize)]
struct RevMeta {
    timestamp: Option<i64>,
}
#[derive(Deserialize)]
struct AuthorLine {
    #[serde(rename = "authorID")]
    author_id: String,
    value: serde_json::Value,
}

/// UTF-16-Einheit mit Attributen (Schlüssel → Wert), die Arbeitsform des Replays.
/// Etherpad zählt Längen wie JavaScript in UTF-16-Einheiten; Emoji belegen zwei.
type Attr = BTreeMap<String, String>;
#[derive(Clone)]
struct Ch {
    c: u16,
    a: Attr,
}
const NL: u16 = 0x0A;
const STAR: u16 = 0x2A;

fn b36(s: &str) -> usize {
    usize::from_str_radix(s, 36).unwrap_or(0)
}

/// Ein Op des Easysync-Formats.
struct Op {
    attribs: Vec<usize>,
    kind: char,
    len: usize,
}

fn parse_ops(s: &str) -> Vec<Op> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut ops = Vec::new();
    while i < b.len() {
        let mut attribs = Vec::new();
        while i < b.len() && b[i] == b'*' {
            i += 1;
            let st = i;
            while i < b.len() && b[i].is_ascii_alphanumeric() { i += 1; }
            attribs.push(b36(&s[st..i]));
        }
        if i < b.len() && b[i] == b'|' {
            i += 1;
            while i < b.len() && b[i].is_ascii_alphanumeric() { i += 1; }
        }
        if i >= b.len() { break; }
        let kind = b[i] as char;
        if !matches!(kind, '+' | '-' | '=') { i += 1; continue; }
        i += 1;
        let st = i;
        while i < b.len() && b[i].is_ascii_alphanumeric() { i += 1; }
        ops.push(Op { attribs, kind, len: b36(&s[st..i]) });
    }
    ops
}

fn attr_apply(target: &mut Attr, nums: &[usize], pool: &Pool, full: bool) {
    if full { target.clear(); }
    for n in nums {
        if let Some((k, v)) = pool.num_to_attrib.get(&n.to_string()) {
            if v.is_empty() { target.remove(k); } else { target.insert(k.clone(), v.clone()); }
        }
    }
}

/// Zeichenliste aus atext (nur `+`-Ops).
fn atext_to_chars(atext: &AText, pool: &Pool) -> Vec<Ch> {
    let chars: Vec<u16> = atext.text.encode_utf16().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut pos = 0;
    for op in parse_ops(&atext.attribs) {
        if op.kind != '+' { continue; }
        let mut a = Attr::new();
        attr_apply(&mut a, &op.attribs, pool, true);
        for _ in 0..op.len {
            if pos < chars.len() { out.push(Ch { c: chars[pos], a: a.clone() }); pos += 1; }
        }
    }
    while pos < chars.len() { out.push(Ch { c: chars[pos], a: Attr::new() }); pos += 1; }
    out
}

/// Wendet ein Changeset `Z:…$bank` auf die Zeichenliste an.
fn apply_changeset(doc: &mut Vec<Ch>, cs: &str, pool: &Pool) -> Result<()> {
    let body = cs.strip_prefix("Z:").ok_or_else(|| anyhow!("kein Z:-Changeset"))?;
    let dollar = body.find('$').ok_or_else(|| anyhow!("kein $ im Changeset"))?;
    let (head, bank) = (&body[..dollar], &body[dollar + 1..]);
    let ops_start = head.find(['+', '-', '=', '*', '|']).unwrap_or(head.len());
    // Kopf: <oldlen>(<|>)<delta>; wir prüfen nur locker
    let ops = parse_ops(&head[ops_start..]);
    let bank: Vec<u16> = bank.encode_utf16().collect();
    let mut out: Vec<Ch> = Vec::with_capacity(doc.len() + bank.len());
    let mut src = 0usize;
    let mut bpos = 0usize;
    for op in ops {
        match op.kind {
            '=' => {
                for _ in 0..op.len {
                    if src < doc.len() {
                        let mut ch = doc[src].clone();
                        if !op.attribs.is_empty() { attr_apply(&mut ch.a, &op.attribs, pool, false); }
                        out.push(ch);
                        src += 1;
                    }
                }
            }
            '-' => { src = (src + op.len).min(doc.len()); }
            '+' => {
                let mut a = Attr::new();
                attr_apply(&mut a, &op.attribs, pool, true);
                for _ in 0..op.len {
                    if bpos < bank.len() { out.push(Ch { c: bank[bpos], a: a.clone() }); bpos += 1; }
                }
            }
            _ => {}
        }
    }
    out.extend_from_slice(&doc[src..]);
    *doc = out;
    Ok(())
}

/// Zeichenliste → Blöcke (Absätze, Überschriften, Listen) mit Marks.
fn chars_to_blocks(chars: &[Ch]) -> Vec<Block> {
    // Zeilen trennen
    let mut lines: Vec<Vec<Ch>> = vec![Vec::new()];
    for ch in chars {
        if ch.c == NL { lines.push(Vec::new()); } else { lines.last_mut().unwrap().push(ch.clone()); }
    }
    if lines.last().map(|l| l.is_empty()).unwrap_or(false) && lines.len() > 1 { lines.pop(); }

    struct Line { kind: LineKind, runs: Vec<(String, Marks)>, align: Option<String> }
    enum LineKind { Para, Heading(u8), List { ordered: bool, level: usize } }

    let mut parsed: Vec<Line> = Vec::new();
    for mut l in lines {
        let mut kind = LineKind::Para;
        let mut align = None;
        if let Some(first) = l.first() {
            if first.c == STAR && first.a.contains_key("lmkr") {
                let a = first.a.clone();
                if let Some(h) = a.get("heading") {
                    let lv: u8 = h.trim_start_matches('h').parse().unwrap_or(1);
                    kind = LineKind::Heading(lv.clamp(1, 6).min(4));
                }
                if let Some(list) = a.get("list") {
                    let ordered = list.starts_with("number");
                    let level: usize = list.trim_start_matches(|c: char| c.is_alphabetic()).parse().unwrap_or(1);
                    kind = LineKind::List { ordered, level: level.max(1) };
                }
                if let Some(al) = a.get("align") { if al != "left" { align = Some(al.clone()); } }
                l.remove(0);
            }
        }
        // Läufe gleicher Marks zusammenfassen
        // Läufe gleicher Marks als UTF-16-Folgen sammeln, dann in Strings wandeln
        let mut units: Vec<(Vec<u16>, Marks)> = Vec::new();
        for ch in l {
            let m = marks_of(&ch.a);
            match units.last_mut() {
                Some((u, lm)) if marks_eq(lm, &m) => u.push(ch.c),
                _ => units.push((vec![ch.c], m)),
            }
        }
        let runs: Vec<(String, Marks)> = units.into_iter().map(|(u, m)| (String::from_utf16_lossy(&u), m)).collect();
        parsed.push(Line { kind, runs, align });
    }

    // Listen verschachteln
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;
    while i < parsed.len() {
        match &parsed[i].kind {
            LineKind::Para => { blocks.push(Block::Para { align: parsed[i].align.clone(), indent: 0, runs: parsed[i].runs.clone() }); i += 1; }
            LineKind::Heading(lv) => { blocks.push(Block::Heading { level: *lv, indent: 0, runs: parsed[i].runs.clone() }); i += 1; }
            LineKind::List { .. } => {
                let (b, next) = build_list(&parsed, i, 1);
                blocks.push(b);
                i = next;
            }
        }
    }
    return blocks;

    fn build_list(lines: &[Line], start: usize, level: usize) -> (Block, usize) {
        let ordered = matches!(lines[start].kind, LineKind::List { ordered: true, .. });
        let mut items: Vec<Vec<Block>> = Vec::new();
        let mut i = start;
        while i < lines.len() {
            match &lines[i].kind {
                LineKind::List { ordered: o, level: lv } if *lv == level && *o == ordered => {
                    items.push(vec![Block::Para { align: None, indent: 0, runs: lines[i].runs.clone() }]);
                    i += 1;
                }
                LineKind::List { level: lv, .. } if *lv > level => {
                    let (sub, next) = build_list(lines, i, *lv);
                    if items.is_empty() { items.push(vec![Block::Para { align: None, indent: 0, runs: vec![] }]); }
                    items.last_mut().unwrap().push(sub);
                    i = next;
                }
                _ => break,
            }
        }
        (Block::List { ordered, items }, i)
    }
}

fn marks_of(a: &Attr) -> Marks {
    let mut m = Marks::default();
    for (k, v) in a {
        match k.as_str() {
            "bold" => m.bold = v != "false" && !v.is_empty(),
            "italic" => m.italic = v != "false" && !v.is_empty(),
            "underline" => m.underline = v != "false" && !v.is_empty(),
            "strikethrough" => m.strike = v != "false" && !v.is_empty(),
            "author" => if !v.is_empty() { m.author = Some(v.clone()) },
            "color" => if !v.is_empty() { m.color = Some(v.clone()) },
            "font-size" => if !v.is_empty() { m.font_size = Some(format!("{}px", v.trim_end_matches("px"))) },
            _ => {
                if let Some(sz) = k.strip_prefix("font-size:") { m.font_size = Some(format!("{sz}px")); }
                if let Some(c) = k.strip_prefix("color:") { m.color = Some(c.to_string()); }
            }
        }
    }
    m
}

fn marks_eq(a: &Marks, b: &Marks) -> bool {
    a.bold == b.bold && a.italic == b.italic && a.underline == b.underline && a.strike == b.strike && a.color == b.color && a.font_size == b.font_size && a.author == b.author
}

/// Baut ein Yjs-Dokument aus Blöcken, liefert (Zustand als Update, HTML, Text).
fn doc_from_blocks(blocks: &[Block]) -> (Vec<u8>, String, String) {
    let doc = Doc::with_options(Options { skip_gc: true, offset_kind: OffsetKind::Utf16, ..Default::default() });
    let frag = doc.get_or_insert_xml_fragment(render::FRAGMENT);
    {
        let mut txn = doc.transact_mut();
        render::write_blocks(&mut txn, &frag, blocks);
    }
    let txn = doc.transact();
    (txn.encode_state_as_update_v1(&StateVector::default()), render::to_html(&txn, &frag), render::to_text(&txn, &frag))
}

fn plain_text(chars: &[Ch]) -> String {
    String::from_utf16_lossy(&chars.iter().map(|c| c.c).collect::<Vec<u16>>())
}

pub struct Report {
    pub pads: usize,
    pub authors: usize,
    pub groups: usize,
    pub snapshots: usize,
    pub text_mismatch: Vec<String>,
    pub replay_mismatch: Vec<String>,
    pub errors: Vec<String>,
}

/// Führt den Import aus. `only` begrenzt auf Pad-Kennungen (leer = alle).
pub fn import(store: &Store, export: &Path, only: &[String], with_history: bool) -> Result<Report> {
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string(export.join("manifest.json")).context("manifest.json")?)?;
    let mut rep = Report { pads: 0, authors: 0, groups: 0, snapshots: 0, text_mismatch: vec![], replay_mismatch: vec![], errors: vec![] };

    // Autoren
    let mut mapper_of: HashMap<String, String> = HashMap::new();
    if let Ok(f) = fs::File::open(export.join("autoren/mapper2author.tsv")) {
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            let mut it = line.split('\t');
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                let mapper = k.trim_start_matches("mapper2author:").to_string();
                let aid = v.trim().trim_matches('"').to_string();
                mapper_of.insert(aid, mapper);
            }
        }
    }
    if let Ok(f) = fs::File::open(export.join("autoren/globalAuthor.jsonl")) {
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            let Ok(a) = serde_json::from_str::<AuthorLine>(&line) else { continue };
            let name = a.value.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let color = a.value.get("colorId").and_then(|v| v.as_i64()).map(|c| c as i32);
            match store.import_author(&a.author_id, mapper_of.get(&a.author_id).map(String::as_str), &name, color) {
                Ok(()) => rep.authors += 1,
                Err(e) => rep.errors.push(format!("Autor {}: {e}", a.author_id)),
            }
        }
    }

    for mp in &manifest.pads_liste {
        if !only.is_empty() && !only.iter().any(|o| o == &mp.pad_id) { continue; }
        let dir = export.join("pads").join(urlencode(&mp.pad_id));
        let r = import_pad(store, mp, &dir, with_history, &mut rep);
        if let Err(e) = r { rep.errors.push(format!("{}: {e}", mp.pad_id)); }
    }
    rep.groups = store.counts().map(|c| c.1 as usize).unwrap_or(0);
    Ok(rep)
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn import_pad(store: &Store, mp: &ManifestPad, dir: &Path, with_history: bool, rep: &mut Report) -> Result<()> {
    let pad: PadJson = serde_json::from_str(&lenient_json(&fs::read_to_string(dir.join("pad.json")).context("pad.json")?))?;
    if let Some(g) = &mp.group_id { store.import_group(g)?; rep.groups += 0; }
    let created_ms = mp.created.as_deref().and_then(parse_iso_ms).unwrap_or(0);
    let last = mp.last_edited_ms.unwrap_or(created_ms);
    let ro = mp.readonly_id.clone().unwrap_or_else(crate::ids::readonly_id);
    store.import_pad(&mp.pad_id, mp.group_id.as_deref(), &ro, created_ms, last)?;

    // Tagesstände aus den Revisionen
    let mut final_chars: Option<Vec<Ch>> = None;
    if with_history {
        let rp = dir.join("revisionen.jsonl");
        if rp.exists() {
            // Etherpad beginnt jedes Pad mit einem Zeilenumbruch (Revision 0 ändert ein Dokument der Länge 1)
            let mut doc: Vec<Ch> = vec![Ch { c: NL, a: Attr::new() }];
            let mut last_day: Option<i64> = None;
            let mut pending: Option<(i64, i64)> = None; // (ts, rev) des letzten Stands des Tages
            let mut snaps = 0;
            let f = fs::File::open(&rp)?;
            let mut lines: Vec<RevLine> = Vec::new();
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                if let Ok(r) = serde_json::from_str::<RevLine>(&lenient_json(&line)) { lines.push(r); }
            }
            lines.sort_by_key(|r| r.rev);
            let n = lines.len();
            for (idx, r) in lines.iter().enumerate() {
                if let Err(e) = apply_changeset(&mut doc, &r.value.changeset, &pad.pool) {
                    rep.errors.push(format!("{} rev {}: {e}", mp.pad_id, r.rev));
                    break;
                }
                let ts = r.value.meta.timestamp.unwrap_or(0);
                let day = ts / 86_400_000;
                let next_day = lines.get(idx + 1).map(|x| x.value.meta.timestamp.unwrap_or(0) / 86_400_000);
                let is_last = idx + 1 == n;
                if next_day != Some(day) && !is_last {
                    // letzter Stand dieses Tages
                    let blocks = chars_to_blocks(&doc);
                    let (state, html, _) = doc_from_blocks(&blocks);
                    store.import_snapshot(&mp.pad_id, ts, &format!("Etherpad-Revision {}", r.rev), &state, &html)?;
                    snaps += 1;
                }
                last_day = Some(day);
                pending = Some((ts, r.rev));
            }
            let _ = (last_day, pending);
            rep.snapshots += snaps;
            final_chars = Some(doc);
        }
    }

    // aktueller Stand aus atext
    let chars = atext_to_chars(&pad.atext, &pad.pool);
    if let Some(fc) = &final_chars {
        let (a, b) = (plain_text(fc), plain_text(&chars));
        if a != b {
            let pos = a.chars().zip(b.chars()).position(|(x, y)| x != y).unwrap_or(a.chars().count().min(b.chars().count()));
            let ca = a.chars().nth(pos).map(|c| format!("U+{:04X}", c as u32)).unwrap_or("Ende".into());
            let cb = b.chars().nth(pos).map(|c| format!("U+{:04X}", c as u32)).unwrap_or("Ende".into());
            rep.replay_mismatch.push(format!("{} (Längen {}/{}, Position {}, {}→{})", mp.pad_id, a.chars().count(), b.chars().count(), pos, ca, cb));
        }
    }
    let blocks = chars_to_blocks(&chars);
    let (state, html, text) = doc_from_blocks(&blocks);
    // Vergleich mit dem API-Export (inhalt.txt): Zeilen ohne Listenpräfixe
    if let Ok(exp) = fs::read_to_string(dir.join("inhalt.txt")) {
        let (ne, nt) = (normalize(&exp), normalize(&text));
        if ne != nt {
            let el: Vec<&str> = ne.lines().collect(); let tl: Vec<&str> = nt.lines().collect();
            let first = el.iter().zip(tl.iter()).position(|(a, b)| a != b).unwrap_or(el.len().min(tl.len()));
            let attrs: Vec<String> = pad.pool.num_to_attrib.values().map(|(k, _)| k.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            // abweichende Zeichen nur als Codepunkte (kein Inhalt)
            let (la, lb) = (el.get(first).copied().unwrap_or(""), tl.get(first).copied().unwrap_or(""));
            let diffs: Vec<String> = la.chars().zip(lb.chars()).filter(|(a, b)| a != b).take(4).map(|(a, b)| format!("U+{:04X}→U+{:04X}", a as u32, b as u32)).collect();
            rep.text_mismatch.push(format!("{} (Zeilen export/neu {}/{}, erste Abweichung Zeile {}, Längen {}/{}, Zeichen {}, Attribute {})", mp.pad_id, el.len(), tl.len(), first + 1, la.chars().count(), lb.chars().count(), diffs.join(" "), attrs.join("|")));
        }
    }
    let seq = store.import_initial_update(&mp.pad_id, &state)?;
    store.import_snapshot(&mp.pad_id, last.max(1), &format!("Migration (Etherpad-Revision {})", pad.head.unwrap_or(0)), &state, &html)?;
    let _ = seq;
    rep.snapshots += 1;
    rep.pads += 1;
    Ok(())
}

/// Normalisierung für den Textvergleich: Listenpräfixe, Zeilenmarker und Whitespace entfernen.
fn normalize(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_start_matches(|c: char| c == '*' || c == '-' || c == ' ' || c == '\t' || c.is_ascii_digit() || c == '.'))
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Ersetzt einsame UTF-16-Surrogate in JSON-Escapes (`\\ud83d` ohne Partner) durch U+FFFD,
/// serde_json lehnt sie sonst ab. Etherpad speichert sie, wenn ein Emoji zerschnitten wurde.
fn lenient_json(raw: &str) -> String {
    let b = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 5 < b.len() && b[i + 1] == b'u' {
            let hex = &raw[i + 2..i + 6];
            if let Ok(cp) = u32::from_str_radix(hex, 16) {
                let is_high = (0xD800..=0xDBFF).contains(&cp);
                let is_low = (0xDC00..=0xDFFF).contains(&cp);
                if is_high {
                    let next_ok = i + 11 < b.len() && &raw[i + 6..i + 8] == "\\u" && u32::from_str_radix(&raw[i + 8..i + 12], 16).map(|n| (0xDC00..=0xDFFF).contains(&n)).unwrap_or(false);
                    if !next_ok { out.push_str("\\ufffd"); i += 6; continue; }
                    out.push_str(&raw[i..i + 12]); i += 12; continue;
                }
                if is_low { out.push_str("\\ufffd"); i += 6; continue; }
            }
        }
        let ch = raw[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn parse_iso_ms(s: &str) -> Option<i64> {
    // "2026-10-23T10:55:13Z" → ms seit Epoche (grob, ohne Zeitzonen außer Z)
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|x| x.split('.').next().unwrap_or("0").parse::<i64>().unwrap_or(0));
    let (h, mi, sec) = (t.next().unwrap_or(0), t.next().unwrap_or(0), t.next().unwrap_or(0));
    // Tage seit 1970 (proleptisch gregorianisch)
    let (y2, m2) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (m2 - 3) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + sec) * 1000)
}
