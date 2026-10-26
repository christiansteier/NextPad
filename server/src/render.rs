//! Übersetzung zwischen dem Yjs-Dokument (ProseMirror-Struktur im XmlFragment "default")
//! und HTML beziehungsweise Text. HTML-Dialekt lehnt sich an Etherpad-Exporte an.
use html_escape::{encode_double_quoted_attribute as attr, encode_text as esc};
use scraper::{ElementRef, Html, Node};
use std::collections::HashMap;
use std::sync::Arc;
use yrs::types::text::YChange;
use yrs::types::{xml::XmlIn, Attrs};
use yrs::{
    Any, GetString, Number, Out, ReadTxn, Text, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment,
    XmlFragmentRef, XmlOut, XmlTextPrelim, XmlTextRef,
};

pub const FRAGMENT: &str = "default";
/// Höchste Verschachtelungstiefe beim Rendern; tiefer wird nur noch flach ausgegeben (Schutz vor Stack-Overflow).
const MAX_DEPTH: usize = 48;

/// CSS-Werte nur aus Positivlisten (Schutz vor CSS-Injektion in Exporten).
pub fn safe_color(v: &str) -> Option<String> {
    let v = v.trim();
    let hex = v.strip_prefix('#').map(|h| (h.len() == 3 || h.len() == 4 || h.len() == 6 || h.len() == 8) && h.chars().all(|c| c.is_ascii_hexdigit())).unwrap_or(false);
    let rgb = v.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')).map(|r| { let p: Vec<&str> = r.split(',').map(str::trim).collect(); p.len() == 3 && p.iter().all(|x| x.parse::<u8>().is_ok()) }).unwrap_or(false);
    let named = matches!(v, "black" | "white" | "red" | "green" | "blue" | "yellow" | "orange" | "purple" | "brown" | "gray" | "grey");
    (hex || rgb || named).then(|| v.to_string())
}
pub fn safe_font_size(v: &str) -> Option<String> {
    let v = v.trim();
    let (num, unit) = v.find(|c: char| !c.is_ascii_digit() && c != '.').map(|i| v.split_at(i)).unwrap_or((v, "px"));
    let n: f32 = num.parse().ok()?;
    (n > 0.0 && n <= 200.0 && matches!(unit, "px" | "pt" | "em" | "rem" | "%")).then(|| format!("{num}{unit}"))
}
pub fn safe_align(v: &str) -> Option<&'static str> {
    match v.trim() { "left" => Some("left"), "center" => Some("center"), "right" => Some("right"), "justify" => Some("justify"), _ => None }
}

/// Aufgabenliste: Häkchen eines taskItem (TipTap speichert true/false, ältere Stände als Text).
fn is_checked<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> bool {
    match el.get_attribute(txn, "checked") {
        Some(Out::Any(Any::Bool(b))) => b,
        Some(Out::Any(Any::String(v))) => &*v == "true",
        _ => false,
    }
}

fn attr_string(out: Option<Out>) -> Option<String> {
    match out {
        Some(Out::Any(Any::String(s))) => Some(s.to_string()),
        Some(Out::Any(Any::Number(Number::Int(n)))) => Some(n.to_string()),
        Some(Out::Any(Any::Number(Number::Float(n)))) => Some(format!("{}", n as i64)),
        Some(Out::Any(a)) => Some(a.to_string()),
        _ => None,
    }
}

fn mark_attr(attrs: &Attrs, mark: &str, key: &str) -> Option<String> {
    match attrs.get(mark) {
        Some(Any::Map(m)) => match m.get(key) {
            Some(Any::String(s)) => Some(s.to_string()),
            Some(Any::Number(Number::Int(n))) => Some(n.to_string()),
            Some(Any::Number(Number::Float(n))) => Some(format!("{}", n)),
            _ => None,
        },
        _ => None,
    }
}

/// Text eines XmlText-Knotens als HTML mit Marks.
fn text_html<T: ReadTxn>(txn: &T, t: &XmlTextRef) -> String {
    let mut out = String::new();
    for d in t.diff(txn, YChange::identity) {
        let s = match d.insert {
            Out::Any(Any::String(s)) => s.to_string(),
            _ => continue,
        };
        let a = d.attributes.map(|b| *b).unwrap_or_default();
        let mut open = String::new();
        let mut close = String::new();
        let mut style = String::new();
        if let Some(c) = mark_attr(&a, "textStyle", "color").and_then(|c| safe_color(&c)) {
            style.push_str(&format!("color:{};", attr(&c)));
        }
        if let Some(fs) = mark_attr(&a, "textStyle", "fontSize").and_then(|f| safe_font_size(&f)) {
            style.push_str(&format!("font-size:{};", attr(&fs)));
        }
        if let Some(author) = mark_attr(&a, "author", "id") {
            open.push_str(&format!("<span data-author=\"{}\">", attr(&author)));
            close.insert_str(0, "</span>");
        }
        if !style.is_empty() {
            open.push_str(&format!("<span style=\"{style}\">"));
            close.insert_str(0, "</span>");
        }
        for (mark, tag) in [("bold", "strong"), ("italic", "em"), ("underline", "u"), ("strike", "s"), ("code", "code")] {
            if a.contains_key(mark) {
                open.push_str(&format!("<{tag}>"));
                close.insert_str(0, &format!("</{tag}>"));
            }
        }
        out.push_str(&open);
        out.push_str(&esc(&s));
        out.push_str(&close);
    }
    out
}

fn element_html<T: ReadTxn>(txn: &T, el: &XmlElementRef, out: &mut String, depth: usize) {
    let tag = el.tag().to_string();
    let inner = |txn: &T, out: &mut String| {
        if depth >= MAX_DEPTH { out.push_str(&esc(&el.get_string(txn))); return; }
        for child in el.children(txn) {
            node_html(txn, &child, out, depth + 1);
        }
    };
    match tag.as_str() {
        "paragraph" => {
            match attr_string(el.get_attribute(txn, "textAlign")).as_deref().and_then(safe_align) {
                Some(a) if a != "left" => out.push_str(&format!("<p style=\"text-align:{a}\">")),
                _ => out.push_str("<p>"),
            }
            inner(txn, out);
            out.push_str("</p>\n");
        }
        "heading" => {
            let level = attr_string(el.get_attribute(txn, "level")).and_then(|l| l.parse::<u8>().ok()).unwrap_or(1).clamp(1, 6);
            out.push_str(&format!("<h{level}>"));
            inner(txn, out);
            out.push_str(&format!("</h{level}>\n"));
        }
        "bulletList" => { out.push_str("<ul>\n"); inner(txn, out); out.push_str("</ul>\n"); }
        "orderedList" => { out.push_str("<ol>\n"); inner(txn, out); out.push_str("</ol>\n"); }
        "listItem" => { out.push_str("<li>"); inner(txn, out); out.push_str("</li>\n"); }
        "taskList" => { out.push_str("<ul data-type=\"taskList\">\n"); inner(txn, out); out.push_str("</ul>\n"); }
        "taskItem" => {
            let c = is_checked(txn, el);
            out.push_str(&format!("<li data-type=\"taskItem\" data-checked=\"{c}\"><input type=\"checkbox\" disabled{}> ", if c { " checked" } else { "" }));
            inner(txn, out);
            out.push_str("</li>\n");
        }
        "blockquote" => { out.push_str("<blockquote>\n"); inner(txn, out); out.push_str("</blockquote>\n"); }
        "codeBlock" => { out.push_str("<pre><code>"); inner(txn, out); out.push_str("</code></pre>\n"); }
        "hardBreak" => out.push_str("<br>"),
        "horizontalRule" => out.push_str("<hr>\n"),
        _ => { out.push_str("<div>"); inner(txn, out); out.push_str("</div>\n"); }
    }
}

fn node_html<T: ReadTxn>(txn: &T, node: &XmlOut, out: &mut String, depth: usize) {
    if depth > MAX_DEPTH { return; }
    match node {
        XmlOut::Element(el) => element_html(txn, el, out, depth),
        XmlOut::Text(t) => out.push_str(&text_html(txn, t)),
        XmlOut::Fragment(f) => {
            for c in f.children(txn) {
                node_html(txn, &c, out, depth + 1);
            }
        }
    }
}

/// Vollständiges HTML-Dokument wie Etherpads getHTML.
pub fn to_html<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String {
    let mut body = String::new();
    for c in frag.children(txn) {
        node_html(txn, &c, &mut body, 0);
    }
    format!("<!DOCTYPE HTML><html><body>{body}</body></html>")
}

fn node_text<T: ReadTxn>(txn: &T, node: &XmlOut, out: &mut String, depth: usize, ordered: Option<&mut usize>) {
    if depth > MAX_DEPTH { return; }
    match node {
        XmlOut::Text(t) => {
            for d in t.diff(txn, YChange::identity) {
                if let Out::Any(Any::String(s)) = d.insert { out.push_str(&s); }
            }
        }
        XmlOut::Fragment(f) => {
            for c in f.children(txn) {
                node_text(txn, &c, out, depth, None);
            }
        }
        XmlOut::Element(el) => {
            let tag = el.tag().to_string();
            match tag.as_str() {
                "bulletList" | "orderedList" | "taskList" => {
                    let mut n = 0usize;
                    for c in el.children(txn) {
                        if tag == "orderedList" { n += 1; }
                        if let XmlOut::Element(li) = &c {
                            let prefix = if tag == "orderedList" { format!("{}{}. ", "  ".repeat(depth), n) } else if tag == "taskList" { format!("{}- [{}] ", "  ".repeat(depth), if is_checked(txn, li) { 'x' } else { ' ' }) } else { format!("{}- ", "  ".repeat(depth)) };
                            out.push_str(&prefix);
                            let mut first = true;
                            for lc in li.children(txn) {
                                if let XmlOut::Element(e) = &lc {
                                    let t = e.tag().to_string();
                                    if t == "bulletList" || t == "orderedList" || t == "taskList" {
                                        node_text(txn, &lc, out, depth + 1, None);
                                        continue;
                                    }
                                    if !first { out.push_str(&"  ".repeat(depth + 1)); }
                                    let mut inner = String::new();
                                    for ic in e.children(txn) { node_text(txn, &ic, &mut inner, depth, None); }
                                    out.push_str(&inner);
                                    out.push('\n');
                                    first = false;
                                } else {
                                    node_text(txn, &lc, out, depth, None);
                                }
                            }
                        }
                    }
                }
                "hardBreak" => out.push('\n'),
                "horizontalRule" => out.push_str("----\n"),
                _ => {
                    for c in el.children(txn) {
                        node_text(txn, &c, out, depth + 1, None);
                    }
                    out.push('\n');
                }
            }
        }
    }
    let _ = ordered;
}

/// Reiner Text wie Etherpads getText: Blöcke zeilenweise, Listen mit Präfix.
pub fn to_text<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String {
    let mut out = String::new();
    for c in frag.children(txn) {
        node_text(txn, &c, &mut out, 0, None);
    }
    out
}

// ---------- Markdown ----------

fn text_md<T: ReadTxn>(txn: &T, t: &XmlTextRef) -> String {
    let mut out = String::new();
    for d in t.diff(txn, YChange::identity) {
        let s = match d.insert { Out::Any(Any::String(s)) => s.to_string(), _ => continue };
        let a = d.attributes.map(|b| *b).unwrap_or_default();
        let (lead, body, trail) = split_ws(&s);
        let mut open = String::new();
        let mut close = String::new();
        if a.contains_key("code") { open.push('`'); close.insert(0, '`'); }
        if a.contains_key("bold") { open.push_str("**"); close.insert_str(0, "**"); }
        if a.contains_key("italic") { open.push('*'); close.insert(0, '*'); }
        if a.contains_key("strike") { open.push_str("~~"); close.insert_str(0, "~~"); }
        if a.contains_key("underline") { open.push_str("<u>"); close.insert_str(0, "</u>"); }
        out.push_str(lead);
        if body.is_empty() { out.push_str(trail); continue; }
        out.push_str(&open); out.push_str(&body.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")); out.push_str(&close); out.push_str(trail);
    }
    out
}

fn split_ws(s: &str) -> (&str, &str, &str) {
    let start = s.len() - s.trim_start().len();
    let end = s.trim_end().len();
    if end <= start { return (s, "", ""); }
    (&s[..start], &s[start..end], &s[end..])
}

fn inline_md<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> String { inline_md_d(txn, el, 0) }

fn inline_md_d<T: ReadTxn>(txn: &T, el: &XmlElementRef, depth: usize) -> String {
    let mut out = String::new();
    if depth > MAX_DEPTH { return out; }
    for c in el.children(txn) {
        match &c {
            XmlOut::Text(t) => out.push_str(&text_md(txn, t)),
            XmlOut::Element(e) if e.tag().as_ref() == "hardBreak" => out.push_str("  \n"),
            XmlOut::Element(e) => out.push_str(&inline_md_d(txn, e, depth + 1)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn block_md<T: ReadTxn>(txn: &T, node: &XmlOut, out: &mut String, depth: usize) {
    if depth > MAX_DEPTH { return; }
    let XmlOut::Element(el) = node else { return };
    let tag = el.tag().to_string();
    match tag.as_str() {
        "paragraph" => { out.push_str(&inline_md(txn, el)); out.push_str("\n\n"); }
        "heading" => {
            let level = attr_string(el.get_attribute(txn, "level")).and_then(|l| l.parse::<usize>().ok()).unwrap_or(1).clamp(1, 6);
            out.push_str(&"#".repeat(level)); out.push(' '); out.push_str(&inline_md(txn, el)); out.push_str("\n\n");
        }
        "bulletList" | "orderedList" | "taskList" => {
            let mut n = 0;
            for c in el.children(txn) {
                let XmlOut::Element(li) = &c else { continue };
                n += 1;
                let prefix = if tag == "orderedList" { format!("{}{}. ", "  ".repeat(depth), n) } else if tag == "taskList" { format!("{}- [{}] ", "  ".repeat(depth), if is_checked(txn, li) { 'x' } else { ' ' }) } else { format!("{}- ", "  ".repeat(depth)) };
                let mut first = true;
                for lc in li.children(txn) {
                    if let XmlOut::Element(e) = &lc {
                        let t = e.tag().to_string();
                        if t == "bulletList" || t == "orderedList" || t == "taskList" { block_md(txn, &lc, out, depth + 1); continue; }
                        if first { out.push_str(&prefix); } else { out.push_str(&"  ".repeat(depth + 1)); }
                        out.push_str(&inline_md(txn, e)); out.push('\n');
                        first = false;
                    }
                }
            }
            if depth == 0 { out.push('\n'); }
        }
        "codeBlock" => { out.push_str("```\n"); out.push_str(&inline_md(txn, el)); out.push_str("\n```\n\n"); }
        "horizontalRule" => out.push_str("---\n\n"),
        _ => { out.push_str(&inline_md(txn, el)); out.push_str("\n\n"); }
    }
}

/// Markdown-Export (CommonMark; Unterstreichung als <u>, Farben und Größen entfallen).
pub fn to_markdown<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String {
    let mut out = String::new();
    for c in frag.children(txn) { block_md(txn, &c, &mut out, 0); }
    out.trim_end().to_string() + "\n"
}

// ---------- Typst (PDF-Satz) ----------

/// Typst-Zeichenkette: alles Inhaltliche geht als String-Literal in den Satz, nie als Markup.
pub fn typst_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 4);
    o.push('"');
    for c in s.chars() {
        match c { '\\' => o.push_str("\\\\"), '"' => o.push_str("\\\""), '\n' => o.push_str("\\n"), '\r' => {}, '\t' => o.push(' '), c if c.is_control() => {}, c => o.push(c) }
    }
    o.push('"');
    o
}

fn typst_color(v: &str) -> Option<String> {
    let v = safe_color(v)?;
    if let Some(h) = v.strip_prefix('#') {
        let full = if h.len() == 3 || h.len() == 4 { h.chars().take(3).flat_map(|c| [c, c]).collect::<String>() } else { h.chars().take(6).collect() };
        return Some(format!("rgb(\"#{full}\")"));
    }
    if let Some(r) = v.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) { return Some(format!("rgb({r})")); }
    Some(match v.as_str() { "brown" => "rgb(\"#8b4513\")".into(), "grey" => "gray".into(), n => n.to_string() })
}

fn text_typst<T: ReadTxn>(txn: &T, t: &XmlTextRef) -> String {
    let mut out = String::new();
    for d in t.diff(txn, YChange::identity) {
        let s = match d.insert { Out::Any(Any::String(s)) => s.to_string(), _ => continue };
        if s.is_empty() { continue; }
        let a = d.attributes.map(|b| *b).unwrap_or_default();
        let mut piece = if a.contains_key("code") { format!("#raw({})", typst_str(&s)) } else { format!("#{}", typst_str(&s)) };
        if a.contains_key("bold") { piece = format!("#strong[{piece}]"); }
        if a.contains_key("italic") { piece = format!("#emph[{piece}]"); }
        if a.contains_key("strike") { piece = format!("#strike[{piece}]"); }
        if a.contains_key("underline") { piece = format!("#underline[{piece}]"); }
        if let Some(c) = mark_attr(&a, "textStyle", "color").and_then(|c| typst_color(&c)) {
            piece = format!("#text(fill: {c})[{piece}]");
        }
        out.push_str(&piece);
    }
    out
}

fn inline_typst<T: ReadTxn>(txn: &T, el: &XmlElementRef, depth: usize) -> String {
    let mut out = String::new();
    if depth > MAX_DEPTH { return out; }
    for c in el.children(txn) {
        match &c {
            XmlOut::Text(t) => out.push_str(&text_typst(txn, t)),
            XmlOut::Element(e) if e.tag().as_ref() == "hardBreak" => out.push_str("#linebreak()"),
            XmlOut::Element(e) => out.push_str(&inline_typst(txn, e, depth + 1)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn aligned<T: ReadTxn>(txn: &T, el: &XmlElementRef, body: String) -> String {
    match attr_string(el.get_attribute(txn, "textAlign")).as_deref().and_then(safe_align) {
        Some("center") => format!("#align(center)[{body}]"),
        Some("right") => format!("#align(right)[{body}]"),
        Some("justify") => format!("#par(justify: true)[{body}]"),
        _ => body,
    }
}

fn block_typst<T: ReadTxn>(txn: &T, node: &XmlOut, out: &mut String, depth: usize) {
    if depth > MAX_DEPTH { return; }
    let XmlOut::Element(el) = node else { return };
    let tag = el.tag().to_string();
    let ind = "  ".repeat(depth);
    match tag.as_str() {
        "paragraph" => {
            let body = inline_typst(txn, el, 0);
            if body.is_empty() { out.push_str("#v(0.4em)\n\n"); } else { out.push_str(&aligned(txn, el, body)); out.push_str("\n\n"); }
        }
        "heading" => {
            let level = attr_string(el.get_attribute(txn, "level")).and_then(|l| l.parse::<usize>().ok()).unwrap_or(1).clamp(1, 6);
            let body = inline_typst(txn, el, 0);
            if body.is_empty() { return; }
            out.push_str(&format!("#heading(level: {level})[{body}]\n\n"));
        }
        "bulletList" | "orderedList" | "taskList" => {
            let marker = if tag == "orderedList" { "+" } else { "-" };
            for c in el.children(txn) {
                let XmlOut::Element(li) = &c else { continue };
                // Aufgabe: gezeichnetes Kästchen vor dem Text, gefüllt, wenn erledigt
                let check = if tag == "taskList" { format!("#box(width: 0.7em, height: 0.7em, stroke: 0.7pt, baseline: 0.08em, fill: {})[] ", if is_checked(txn, li) { "black" } else { "none" }) } else { String::new() };
                let mut first = true;
                for lc in li.children(txn) {
                    let XmlOut::Element(e) = &lc else { continue };
                    let t = e.tag().to_string();
                    if t == "bulletList" || t == "orderedList" || t == "taskList" { if first { out.push_str(&format!("{ind}{marker} #[]\n")); first = false; } block_typst(txn, &lc, out, depth + 1); continue; }
                    let body = inline_typst(txn, e, 0);
                    let body = if body.is_empty() { "#[]".to_string() } else { body };
                    if first { out.push_str(&format!("{ind}{marker} {check}{body}\n")); first = false; } else { out.push_str(&format!("{ind}  {body}\n")); }
                }
            }
            if depth == 0 { out.push('\n'); }
        }
        "codeBlock" => {
            let mut text = String::new();
            for c in el.children(txn) { if let XmlOut::Text(t) = &c { text.push_str(&t.get_string(txn)); } }
            out.push_str(&format!("#raw(block: true, {})\n\n", typst_str(&text)));
        }
        "horizontalRule" => out.push_str("#line(length: 100%, stroke: 0.5pt + luma(170))\n\n"),
        _ => { let body = inline_typst(txn, el, 0); if !body.is_empty() { out.push_str(&body); out.push_str("\n\n"); } }
    }
}

/// Pad-Inhalt als Typst-Markup (Inhalte nur als String-Literale, keine Ausführung von Nutzertext).
pub fn to_typst<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String {
    let mut out = String::new();
    for c in frag.children(txn) { block_typst(txn, &c, &mut out, 0); }
    out
}

// ---------- Import ----------

#[derive(Clone, Default, Debug)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub color: Option<String>,
    pub font_size: Option<String>,
    pub author: Option<String>,
}

impl Marks {
    fn to_attrs(&self) -> Attrs {
        let mut a = Attrs::new();
        let empty = || Any::Map(Arc::new(HashMap::new()));
        if self.bold { a.insert("bold".into(), empty()); }
        if self.italic { a.insert("italic".into(), empty()); }
        if self.underline { a.insert("underline".into(), empty()); }
        if self.strike { a.insert("strike".into(), empty()); }
        if self.code { a.insert("code".into(), empty()); }
        if self.color.is_some() || self.font_size.is_some() {
            let mut m = HashMap::new();
            if let Some(c) = &self.color { m.insert("color".to_string(), Any::String(c.as_str().into())); }
            if let Some(f) = &self.font_size { m.insert("fontSize".to_string(), Any::String(f.as_str().into())); }
            a.insert("textStyle".into(), Any::Map(Arc::new(m)));
        }
        if let Some(id) = &self.author {
            let mut m = HashMap::new();
            m.insert("id".to_string(), Any::String(id.as_str().into()));
            a.insert("author".into(), Any::Map(Arc::new(m)));
        }
        a
    }
}

/// Ein Block, wie er ins Dokument geschrieben wird.
#[derive(Clone, Debug)]
pub enum Block {
    Para { align: Option<String>, runs: Vec<(String, Marks)> },
    Heading { level: u8, runs: Vec<(String, Marks)> },
    List { ordered: bool, items: Vec<Vec<Block>> },
    /// Aufgabenliste: je Eintrag erledigt ja/nein und Inhalt
    TaskList { items: Vec<(bool, Vec<Block>)> },
}

fn style_props(style: &str) -> (Option<String>, Option<String>) {
    let mut color = None;
    let mut size = None;
    for decl in style.split(';') {
        let mut kv = decl.splitn(2, ':');
        let k = kv.next().unwrap_or("").trim().to_ascii_lowercase();
        let v = kv.next().unwrap_or("").trim().to_string();
        if k == "color" { color = safe_color(&v); }
        if k == "font-size" { size = safe_font_size(&v); }
    }
    (color, size)
}

fn collect_inline(el: ElementRef, marks: &Marks, runs: &mut Vec<(String, Marks)>, blocks: &mut Vec<Block>, cur_align: &mut Option<String>) {
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                let s = t.text.replace('\n', " ");
                if s.trim().is_empty() {
                    // Whitespace zwischen Blöcken (Zeilenumbrüche im Export) verwerfen,
                    // Whitespace zwischen Inline-Elementen behalten
                    let is_block = |n: Option<ego_tree::NodeRef<Node>>| n.and_then(|n| match n.value() {
                        Node::Element(e) => Some(matches!(e.name().to_ascii_lowercase().as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "blockquote" | "pre" | "br" | "hr")),
                        _ => None,
                    }).unwrap_or(true);
                    if is_block(child.prev_sibling()) || is_block(child.next_sibling()) || runs.is_empty() { continue; }
                }
                if !s.is_empty() { runs.push((s, marks.clone())); }
            }
            Node::Element(e) => {
                let cel = ElementRef::wrap(child).unwrap();
                let name = e.name().to_ascii_lowercase();
                let mut m = marks.clone();
                match name.as_str() {
                    "b" | "strong" => m.bold = true,
                    "i" | "em" => m.italic = true,
                    "u" | "ins" => m.underline = true,
                    "s" | "strike" | "del" => m.strike = true,
                    "code" => m.code = true,
                    "br" => { runs.push(("\n".into(), marks.clone())); continue; }
                    "font" => { if let Some(c) = e.attr("color").and_then(safe_color) { m.color = Some(c); } }
                    _ => {}
                }
                if let Some(st) = e.attr("style") {
                    let (c, f) = style_props(st);
                    if c.is_some() { m.color = c; }
                    if f.is_some() { m.font_size = f; }
                }
                if let Some(a) = e.attr("data-author") { m.author = Some(a.to_string()); }
                // Etherpad-Plugin-Klassen: font-size:18, color:red, heading classes
                if let Some(cls) = e.attr("class") {
                    for c in cls.split_whitespace() {
                        if let Some(v) = c.strip_prefix("font-size:") { m.font_size = safe_font_size(&format!("{v}px")); }
                        if let Some(v) = c.strip_prefix("color:") { m.color = safe_color(v); }
                    }
                }
                if matches!(name.as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "blockquote" | "pre") {
                    // Block innerhalb von Inline: aktuellen Lauf abschließen, Block separat
                    flush_runs(runs, blocks, cur_align.take());
                    blocks.extend(parse_block(cel, &m));
                } else {
                    collect_inline(cel, &m, runs, blocks, cur_align);
                }
            }
            _ => {}
        }
    }
}

fn flush_runs(runs: &mut Vec<(String, Marks)>, blocks: &mut Vec<Block>, align: Option<String>) {
    if runs.is_empty() { return; }
    // Zeilenumbrüche aus <br> trennen Absätze (Etherpad-Export)
    let mut current: Vec<(String, Marks)> = Vec::new();
    let mut pending_break = false;
    for (s, m) in runs.drain(..) {
        if s == "\n" {
            blocks.push(Block::Para { align: align.clone(), runs: std::mem::take(&mut current) });
            pending_break = true;
        } else {
            current.push((s, m));
            pending_break = false;
        }
    }
    if !current.is_empty() || !pending_break {
        blocks.push(Block::Para { align, runs: current });
    }
}

fn parse_block(el: ElementRef, marks: &Marks) -> Vec<Block> {
    let name = el.value().name().to_ascii_lowercase();
    let align = el.value().attr("style").and_then(|s| {
        s.split(';').find_map(|d| {
            let mut kv = d.splitn(2, ':');
            (kv.next()?.trim() == "text-align").then(|| kv.next().and_then(safe_align).map(str::to_string)).flatten()
        })
    });
    let mut blocks = Vec::new();
    match name.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level: u8 = name[1..].parse().unwrap_or(1);
            let mut runs = Vec::new();
            let mut a = None;
            collect_inline(el, marks, &mut runs, &mut blocks, &mut a);
            blocks.push(Block::Heading { level: level.min(4), runs });
        }
        "ul" | "ol" => {
            // Aufgabenliste aus dem eigenen Export (data-type) oder mit Kontrollkästchen im Eintrag
            let is_input_box = |e: ElementRef| e.value().name().eq_ignore_ascii_case("input") && e.value().attr("type").map(|t| t.eq_ignore_ascii_case("checkbox")).unwrap_or(false);
            let li_box = |li: ElementRef| -> Option<bool> { li.descendants().filter_map(ElementRef::wrap).find(|e| is_input_box(*e)).map(|b| b.value().attr("checked").is_some()) };
            let task = name == "ul" && (el.value().attr("data-type") == Some("taskList")
                || el.children().filter_map(ElementRef::wrap).any(|li| li.value().attr("data-checked").is_some() || li_box(li).is_some()));
            if task {
                let mut items = Vec::new();
                for li in el.children().filter_map(ElementRef::wrap) {
                    if !li.value().name().eq_ignore_ascii_case("li") { continue; }
                    let checked = li.value().attr("data-checked") == Some("true") || li_box(li).unwrap_or(false);
                    let mut runs = Vec::new();
                    let mut inner = Vec::new();
                    let mut a = None;
                    collect_inline(li, marks, &mut runs, &mut inner, &mut a);
                    let mut item = Vec::new();
                    if !runs.is_empty() { item.push(Block::Para { align: None, runs }); }
                    item.extend(inner);
                    if item.is_empty() { item.push(Block::Para { align: None, runs: vec![] }); }
                    items.push((checked, item));
                }
                blocks.push(Block::TaskList { items });
                return blocks;
            }
            let mut items = Vec::new();
            for li in el.children().filter_map(ElementRef::wrap) {
                if li.value().name().eq_ignore_ascii_case("li") {
                    let mut runs = Vec::new();
                    let mut inner = Vec::new();
                    let mut a = None;
                    collect_inline(li, marks, &mut runs, &mut inner, &mut a);
                    let mut item = Vec::new();
                    if !runs.is_empty() { item.push(Block::Para { align: None, runs }); }
                    item.extend(inner);
                    if item.is_empty() { item.push(Block::Para { align: None, runs: vec![] }); }
                    items.push(item);
                }
            }
            blocks.push(Block::List { ordered: name == "ol", items });
        }
        "pre" => {
            let text = el.text().collect::<String>();
            blocks.push(Block::Para { align: None, runs: vec![(text, Marks { code: true, ..marks.clone() })] });
        }
        _ => {
            let mut runs = Vec::new();
            let mut a = align;
            collect_inline(el, marks, &mut runs, &mut blocks, &mut a);
            let before = blocks.len();
            flush_runs(&mut runs, &mut blocks, a.clone());
            if blocks.len() == before && name == "p" {
                blocks.push(Block::Para { align: a, runs: vec![] }); // leerer Absatz bleibt erhalten
            }
        }
    }
    blocks
}

fn write_runs(txn: &mut TransactionMut, el: &XmlElementRef, runs: &[(String, Marks)]) {
    if runs.is_empty() { return; }
    let t: XmlTextRef = el.push_back(txn, XmlTextPrelim::new(""));
    let mut idx = 0u32;
    for (s, m) in runs {
        t.insert_with_attributes(txn, idx, s, m.to_attrs());
        idx += s.encode_utf16().count() as u32;
    }
}

fn write_block(txn: &mut TransactionMut, parent: &XmlElementRef, b: &Block) {
    match b {
        Block::Para { align, runs } => {
            let p: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new("paragraph", Vec::<XmlIn>::new()));
            if let Some(a) = align { p.insert_attribute(txn, "textAlign", Any::from(a.as_str())); }
            write_runs(txn, &p, runs);
        }
        Block::Heading { level, runs } => {
            let h: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new("heading", Vec::<XmlIn>::new()));
            h.insert_attribute(txn, "level", Any::from(*level as f64));
            write_runs(txn, &h, runs);
        }
        Block::List { ordered, items } => {
            let l: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new(if *ordered { "orderedList" } else { "bulletList" }, Vec::<XmlIn>::new()));
            for item in items {
                let li: XmlElementRef = l.push_back(txn, XmlElementPrelim::new("listItem", Vec::<XmlIn>::new()));
                for ib in item { write_block(txn, &li, ib); }
            }
        }
        Block::TaskList { items } => {
            let l: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new("taskList", Vec::<XmlIn>::new()));
            for (checked, item) in items {
                let li: XmlElementRef = l.push_back(txn, XmlElementPrelim::new("taskItem", Vec::<XmlIn>::new()));
                li.insert_attribute(txn, "checked", Any::Bool(*checked));
                for ib in item { write_block(txn, &li, ib); }
            }
        }
    }
}

fn write_block_root(txn: &mut TransactionMut, frag: &XmlFragmentRef, b: &Block) {
    // Wurzel ist ein Fragment: gleiche Logik, anderer Elterntyp
    match b {
        Block::Para { align, runs } => {
            let p: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new("paragraph", Vec::<XmlIn>::new()));
            if let Some(a) = align { p.insert_attribute(txn, "textAlign", Any::from(a.as_str())); }
            write_runs(txn, &p, runs);
        }
        Block::Heading { level, runs } => {
            let h: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new("heading", Vec::<XmlIn>::new()));
            h.insert_attribute(txn, "level", Any::from(*level as f64));
            write_runs(txn, &h, runs);
        }
        Block::List { ordered, items } => {
            let l: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new(if *ordered { "orderedList" } else { "bulletList" }, Vec::<XmlIn>::new()));
            for item in items {
                let li: XmlElementRef = l.push_back(txn, XmlElementPrelim::new("listItem", Vec::<XmlIn>::new()));
                for ib in item { write_block(txn, &li, ib); }
            }
        }
        Block::TaskList { items } => {
            let l: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new("taskList", Vec::<XmlIn>::new()));
            for (checked, item) in items {
                let li: XmlElementRef = l.push_back(txn, XmlElementPrelim::new("taskItem", Vec::<XmlIn>::new()));
                li.insert_attribute(txn, "checked", Any::Bool(*checked));
                for ib in item { write_block(txn, &li, ib); }
            }
        }
    }
}

/// Schreibt Blöcke an das Ende des Fragments.
pub fn write_blocks(txn: &mut TransactionMut, frag: &XmlFragmentRef, blocks: &[Block]) {
    for b in blocks { write_block_root(txn, frag, b); }
}

fn clear(txn: &mut TransactionMut, frag: &XmlFragmentRef) {
    let n = frag.len(txn);
    if n > 0 { frag.remove_range(txn, 0, n); }
}

/// Ersetzt den Inhalt durch reinen Text (Zeilen → Absätze).
pub fn set_text(txn: &mut TransactionMut, frag: &XmlFragmentRef, text: &str) {
    clear(txn, frag);
    let lines: Vec<&str> = text.split('\n').collect();
    let lines = if lines.last() == Some(&"") && lines.len() > 1 { &lines[..lines.len() - 1] } else { &lines[..] };
    for line in lines {
        let runs = if line.is_empty() { vec![] } else { vec![(line.to_string(), Marks::default())] };
        write_block_root(txn, frag, &Block::Para { align: None, runs });
    }
    if lines.is_empty() {
        write_block_root(txn, frag, &Block::Para { align: None, runs: vec![] });
    }
}

/// Ersetzt den Inhalt durch importiertes HTML (Etherpad-Dialekt und eigener Export).
pub fn set_html(txn: &mut TransactionMut, frag: &XmlFragmentRef, html: &str) {
    clear(txn, frag);
    let doc = Html::parse_document(html);
    let body = doc.select(&scraper::Selector::parse("body").unwrap()).next();
    let mut blocks: Vec<Block> = Vec::new();
    if let Some(body) = body {
        let mut runs = Vec::new();
        let mut a = None;
        collect_inline(body, &Marks::default(), &mut runs, &mut blocks, &mut a);
        flush_runs(&mut runs, &mut blocks, a);
    }
    // leere Absätze am Ende, die nur vom Parser stammen, entfernen
    while blocks.len() > 1 {
        if let Some(Block::Para { runs, .. }) = blocks.last() {
            if runs.iter().all(|(s, _)| s.trim().is_empty()) { blocks.pop(); continue; }
        }
        break;
    }
    if blocks.is_empty() { blocks.push(Block::Para { align: None, runs: vec![] }); }
    for b in &blocks { write_block_root(txn, frag, b); }
}
