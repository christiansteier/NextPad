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

/// Bewertung als kurzer Text, z. B. „👍 3 · 👎 1“.
fn rating_text<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> String {
    let (up, down) = crate::bausteine::tally(&crate::bausteine::votes(txn, &crate::bausteine::rating_id(txn, el)));
    format!("👍 {up} · 👎 {down}")
}

/// Überschriften des ganzen Pads (für das Inhaltsverzeichnis in Text, Markdown und PDF).
fn doc_headings<T: ReadTxn>(txn: &T) -> Vec<(u8, String)> {
    txn.get_xml_fragment(FRAGMENT).map(|f| crate::bausteine::headings(txn, &f)).unwrap_or_default()
}

/// Zellen einer Tabellenzeile: (Kopfzelle?, Element)
fn row_cells<T: ReadTxn>(txn: &T, row: &XmlOut) -> Vec<(bool, XmlElementRef)> {
    let XmlOut::Element(r) = row else { return Vec::new() };
    r.children(txn).filter_map(|c| match c { XmlOut::Element(e) => Some((e.tag().as_ref() == "tableHeader", e)), _ => None }).collect()
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

/// Einrückstufe eines Absatzes oder einer Überschrift (0 bis 8), vom Editor als Attribut `indent` gesetzt.
fn indent_level<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> usize {
    attr_string(el.get_attribute(txn, "indent")).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0).min(8)
}

/// Stil-Attribut für Ausrichtung und Einrückung, leer ohne beides.
fn block_style<T: ReadTxn>(txn: &T, el: &XmlElementRef) -> String {
    let mut st = String::new();
    if let Some(a) = attr_string(el.get_attribute(txn, "textAlign")).as_deref().and_then(safe_align) {
        if a != "left" { st.push_str(&format!("text-align:{a};")); }
    }
    let n = indent_level(txn, el);
    if n > 0 { st.push_str(&format!("padding-left:{}em;", n * 2)); }
    if st.is_empty() { st } else { format!(" style=\"{}\"", st.trim_end_matches(';')) }
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

/// Zustand beim HTML-Export: Überschriften fürs Inhaltsverzeichnis und ihre Sprungmarken.
struct HtmlCtx { headings: Vec<(u8, String)>, next_heading: usize, in_table: usize, toc: &'static str }

fn toc_html(cx: &HtmlCtx, out: &mut String) {
    out.push_str(&format!("<nav data-type=\"toc\" class=\"toc\"><div class=\"toc-title\">{}</div><ol>", cx.toc));
    let min = cx.headings.iter().map(|h| h.0).min().unwrap_or(1);
    for (i, (level, text)) in cx.headings.iter().enumerate() {
        out.push_str(&format!("<li class=\"lvl{}\"><a href=\"#abschnitt-{}\">{}</a></li>", level - min + 1, i + 1, esc(text)));
    }
    out.push_str("</ol></nav>\n");
}

fn element_html<T: ReadTxn>(txn: &T, el: &XmlElementRef, out: &mut String, depth: usize, cx: &mut HtmlCtx) {
    let tag = el.tag().to_string();
    let inner = |txn: &T, out: &mut String, cx: &mut HtmlCtx| {
        if depth >= MAX_DEPTH { out.push_str(&esc(&el.get_string(txn))); return; }
        for child in el.children(txn) {
            node_html(txn, &child, out, depth + 1, cx);
        }
    };
    match tag.as_str() {
        "paragraph" => {
            out.push_str(&format!("<p{}>", block_style(txn, el)));
            inner(txn, out, cx);
            out.push_str("</p>\n");
        }
        "heading" => {
            let level = attr_string(el.get_attribute(txn, "level")).and_then(|l| l.parse::<u8>().ok()).unwrap_or(1).clamp(1, 6);
            // Sprungmarke in derselben Zählung wie bausteine::headings: ohne leere und ohne solche in Tabellen
            if cx.in_table == 0 && !crate::bausteine::plain_text(txn, el).trim().is_empty() {
                cx.next_heading += 1;
                out.push_str(&format!("<h{level} id=\"abschnitt-{}\"{}>", cx.next_heading, block_style(txn, el)));
            } else {
                out.push_str(&format!("<h{level}{}>", block_style(txn, el)));
            }
            inner(txn, out, cx);
            out.push_str(&format!("</h{level}>\n"));
        }
        "bulletList" => { out.push_str("<ul>\n"); inner(txn, out, cx); out.push_str("</ul>\n"); }
        "orderedList" => { out.push_str("<ol>\n"); inner(txn, out, cx); out.push_str("</ol>\n"); }
        "listItem" => { out.push_str("<li>"); inner(txn, out, cx); out.push_str("</li>\n"); }
        "taskList" => { out.push_str("<ul data-type=\"taskList\">\n"); inner(txn, out, cx); out.push_str("</ul>\n"); }
        "taskItem" => {
            let c = is_checked(txn, el);
            out.push_str(&format!("<li data-type=\"taskItem\" data-checked=\"{c}\"><input type=\"checkbox\" disabled{}> ", if c { " checked" } else { "" }));
            inner(txn, out, cx);
            out.push_str("</li>\n");
        }
        "blockquote" => { out.push_str("<blockquote>\n"); inner(txn, out, cx); out.push_str("</blockquote>\n"); }
        "codeBlock" => { out.push_str("<pre><code>"); inner(txn, out, cx); out.push_str("</code></pre>\n"); }
        "table" => { cx.in_table += 1; out.push_str("<table>\n"); inner(txn, out, cx); out.push_str("</table>\n"); cx.in_table -= 1; }
        "tableRow" => { out.push_str("<tr>"); inner(txn, out, cx); out.push_str("</tr>\n"); }
        "tableHeader" | "tableCell" => {
            let t = if tag == "tableHeader" { "th" } else { "td" };
            let span = |k: &str| attr_string(el.get_attribute(txn, k)).and_then(|v| v.parse::<u32>().ok()).filter(|n| *n > 1 && *n <= 100);
            out.push_str(&format!("<{t}"));
            if let Some(n) = span("colspan") { out.push_str(&format!(" colspan=\"{n}\"")); }
            if let Some(n) = span("rowspan") { out.push_str(&format!(" rowspan=\"{n}\"")); }
            out.push('>'); inner(txn, out, cx); out.push_str(&format!("</{t}>"));
        }
        "rating" => {
            let id = crate::bausteine::rating_id(txn, el);
            let v = crate::bausteine::votes(txn, &id);
            let (up, down) = crate::bausteine::tally(&v);
            out.push_str(&format!("<span data-type=\"rating\" class=\"rating\" data-rating=\"{}\" data-votes=\"{}\">👍 {up} · 👎 {down}</span>", attr(&id), attr(&crate::bausteine::votes_attr(&v))));
        }
        "tableOfContents" => toc_html(cx, out),
        "hardBreak" => out.push_str("<br>"),
        "horizontalRule" => out.push_str("<hr>\n"),
        _ => { out.push_str("<div>"); inner(txn, out, cx); out.push_str("</div>\n"); }
    }
}

fn node_html<T: ReadTxn>(txn: &T, node: &XmlOut, out: &mut String, depth: usize, cx: &mut HtmlCtx) {
    if depth > MAX_DEPTH { return; }
    match node {
        XmlOut::Element(el) => element_html(txn, el, out, depth, cx),
        XmlOut::Text(t) => out.push_str(&text_html(txn, t)),
        XmlOut::Fragment(f) => {
            for c in f.children(txn) {
                node_html(txn, &c, out, depth + 1, cx);
            }
        }
    }
}

/// Titel des Inhaltsverzeichnisses, wo keine Sprache bekannt ist (Speicherung, Etherpad-API).
pub const VERZEICHNIS: &str = "Inhaltsverzeichnis";

/// Vollständiges HTML-Dokument wie Etherpads getHTML.
pub fn to_html<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String { to_html_mit(txn, frag, VERZEICHNIS) }

/// HTML mit eigenem Titel des Inhaltsverzeichnisses (Export in der Sprache der Anfrage).
pub fn to_html_mit<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef, toc: &'static str) -> String {
    let mut body = String::new();
    let mut cx = HtmlCtx { headings: crate::bausteine::headings(txn, frag), next_heading: 0, in_table: 0, toc };
    for c in frag.children(txn) {
        node_html(txn, &c, &mut body, 0, &mut cx);
    }
    format!("<!DOCTYPE HTML><html><body>{body}</body></html>")
}

fn node_text<T: ReadTxn>(txn: &T, toc: &str, node: &XmlOut, out: &mut String, depth: usize, ordered: Option<&mut usize>) {
    if depth > MAX_DEPTH { return; }
    match node {
        XmlOut::Text(t) => {
            for d in t.diff(txn, YChange::identity) {
                if let Out::Any(Any::String(s)) = d.insert { out.push_str(&s); }
            }
        }
        XmlOut::Fragment(f) => {
            for c in f.children(txn) {
                node_text(txn, toc, &c, out, depth, None);
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
                                        node_text(txn, toc, &lc, out, depth + 1, None);
                                        continue;
                                    }
                                    if !first { out.push_str(&"  ".repeat(depth + 1)); }
                                    let mut inner = String::new();
                                    for ic in e.children(txn) { node_text(txn, toc, &ic, &mut inner, depth, None); }
                                    out.push_str(&inner);
                                    out.push('\n');
                                    first = false;
                                } else {
                                    node_text(txn, toc, &lc, out, depth, None);
                                }
                            }
                        }
                    }
                }
                "table" => {
                    for row in el.children(txn) {
                        let cells: Vec<String> = row_cells(txn, &row).iter().map(|(_, c)| {
                            let mut t = String::new();
                            for b in c.children(txn) { node_text(txn, toc, &b, &mut t, depth + 1, None); }
                            t.trim_end().replace('\n', " / ")
                        }).collect();
                        out.push_str(&cells.join(" | ")); out.push('\n');
                    }
                }
                "rating" => out.push_str(&rating_text(txn, el)),
                "tableOfContents" => {
                    out.push_str(toc); out.push('\n');
                    let hs = doc_headings(txn);
                    let min = hs.iter().map(|h| h.0).min().unwrap_or(1);
                    for (l, t) in hs { out.push_str(&format!("{}- {t}\n", "  ".repeat((l - min) as usize))); }
                }
                "hardBreak" => out.push('\n'),
                "horizontalRule" => out.push_str("----\n"),
                _ => {
                    if tag == "paragraph" || tag == "heading" { out.push_str(&"    ".repeat(indent_level(txn, el))); }
                    for c in el.children(txn) {
                        node_text(txn, toc, &c, out, depth + 1, None);
                    }
                    out.push('\n');
                }
            }
        }
    }
    let _ = ordered;
}

/// Reiner Text wie Etherpads getText: Blöcke zeilenweise, Listen mit Präfix.
pub fn to_text<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String { to_text_mit(txn, frag, VERZEICHNIS) }

pub fn to_text_mit<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef, toc: &str) -> String {
    let mut out = String::new();
    for c in frag.children(txn) {
        node_text(txn, toc, &c, &mut out, 0, None);
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
            XmlOut::Element(e) if e.tag().as_ref() == "rating" => out.push_str(&rating_text(txn, e)),
            XmlOut::Element(e) => out.push_str(&inline_md_d(txn, e, depth + 1)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn block_md<T: ReadTxn>(txn: &T, toc: &str, node: &XmlOut, out: &mut String, depth: usize) {
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
                        if t == "bulletList" || t == "orderedList" || t == "taskList" { block_md(txn, toc, &lc, out, depth + 1); continue; }
                        if first { out.push_str(&prefix); } else { out.push_str(&"  ".repeat(depth + 1)); }
                        out.push_str(&inline_md(txn, e)); out.push('\n');
                        first = false;
                    }
                }
            }
            if depth == 0 { out.push('\n'); }
        }
        "codeBlock" => { out.push_str("```\n"); out.push_str(&inline_md(txn, el)); out.push_str("\n```\n\n"); }
        "table" => {
            // GFM-Tabelle: erste Zeile ist der Kopf; mehrere Absätze einer Zelle mit <br>
            let rows: Vec<Vec<String>> = el.children(txn).map(|r| row_cells(txn, &r).iter().map(|(_, c)| {
                c.children(txn).filter_map(|b| match b { XmlOut::Element(e) => Some(inline_md(txn, &e)), _ => None }).collect::<Vec<_>>().join("<br>").replace('|', "\\|").replace('\n', " ")
            }).collect()).filter(|r: &Vec<String>| !r.is_empty()).collect();
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
            if cols == 0 { return; }
            for (i, r) in rows.iter().enumerate() {
                let mut cells = r.clone(); cells.resize(cols, String::new());
                out.push_str(&format!("| {} |\n", cells.join(" | ")));
                if i == 0 { out.push_str(&format!("|{}\n", " --- |".repeat(cols))); }
            }
            out.push('\n');
        }
        "tableOfContents" => {
            let hs = doc_headings(txn);
            if hs.is_empty() { return; }
            out.push_str(&format!("**{toc}**\n\n"));
            let min = hs.iter().map(|h| h.0).min().unwrap_or(1);
            for (l, t) in hs { out.push_str(&format!("{}- {}\n", "  ".repeat((l - min) as usize), t.replace('&', "&amp;").replace('<', "&lt;"))); }
            out.push('\n');
        }
        "horizontalRule" => out.push_str("---\n\n"),
        _ => { out.push_str(&inline_md(txn, el)); out.push_str("\n\n"); }
    }
}

/// Markdown-Export (CommonMark; Unterstreichung als <u>, Farben und Größen entfallen).
pub fn to_markdown<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String { to_markdown_mit(txn, frag, VERZEICHNIS) }

pub fn to_markdown_mit<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef, toc: &str) -> String {
    let mut out = String::new();
    for c in frag.children(txn) { block_md(txn, toc, &c, &mut out, 0); }
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
            XmlOut::Element(e) if e.tag().as_ref() == "rating" => {
                // keine Emoji-Schrift im Satz: Daumen als dieselben Linien-Symbole wie im Editor, in einer Pille
                let (up, down) = crate::bausteine::tally(&crate::bausteine::votes(txn, &crate::bausteine::rating_id(txn, e)));
                let thumb = |paths: &str| format!("#box(baseline: 0.12em, image(bytes({}), height: 0.95em))", typst_str(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"#)));
                out.push_str(&format!("#box(stroke: 0.5pt + luma(150), inset: (x: 3pt, y: 1.5pt), outset: (y: 1pt), radius: 5pt)[#text(size: 0.85em)[{}#h(2pt)#{}#h(5pt){}#h(2pt)#{}]]",
                    thumb(THUMB_UP), typst_str(&up.to_string()), thumb(THUMB_DOWN), typst_str(&down.to_string())));
            }
            XmlOut::Element(e) => out.push_str(&inline_typst(txn, e, depth + 1)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn aligned<T: ReadTxn>(txn: &T, el: &XmlElementRef, body: String) -> String {
    let body = match attr_string(el.get_attribute(txn, "textAlign")).as_deref().and_then(safe_align) {
        Some("center") => format!("#align(center)[{body}]"),
        Some("right") => format!("#align(right)[{body}]"),
        Some("justify") => format!("#par(justify: true)[{body}]"),
        _ => body,
    };
    match indent_level(txn, el) {
        0 => body,
        n => format!("#pad(left: {}em)[{body}]", n * 2),
    }
}

fn block_typst<T: ReadTxn>(txn: &T, toc: &str, node: &XmlOut, out: &mut String, depth: usize) {
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
            let h = format!("#heading(level: {level})[{body}]");
            match indent_level(txn, el) {
                0 => out.push_str(&h),
                n => out.push_str(&format!("#pad(left: {}em)[{h}]", n * 2)),
            }
            out.push_str("\n\n");
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
                    if t == "bulletList" || t == "orderedList" || t == "taskList" { if first { out.push_str(&format!("{ind}{marker} #[]\n")); first = false; } block_typst(txn, toc, &lc, out, depth + 1); continue; }
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
        "table" => {
            let rows: Vec<Vec<(bool, XmlElementRef)>> = el.children(txn).map(|r| row_cells(txn, &r)).filter(|r| !r.is_empty()).collect();
            let cols = rows.iter().map(|r| r.iter().map(|(_, c)| attr_string(c.get_attribute(txn, "colspan")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 100)).sum::<usize>()).max().unwrap_or(0);
            if cols == 0 { return; }
            let header = rows.first().is_some_and(|r| r.iter().all(|(h, _)| *h));
            let cell = |c: &XmlElementRef, head: bool| {
                let mut body = String::new();
                for b in c.children(txn) { block_typst(txn, toc, &b, &mut body, depth + 1); }
                let body = body.trim_end().to_string();
                let body = if head { format!("#strong[{body}]") } else { body };
                let span = |k: &str| attr_string(c.get_attribute(txn, k)).and_then(|v| v.parse::<usize>().ok()).filter(|n| *n > 1 && *n <= 100);
                match (span("colspan"), span("rowspan")) {
                    (None, None) => format!("[{body}]"),
                    (cs, rs) => format!("table.cell(colspan: {}, rowspan: {})[{body}]", cs.unwrap_or(1), rs.unwrap_or(1)),
                }
            };
            out.push_str(&format!("#table(columns: (1fr,) * {cols}, stroke: 0.5pt + luma(150), inset: 5pt, align: left{},\n", if header { ", fill: (_, y) => if y == 0 { luma(235) }" } else { "" }));
            for (i, r) in rows.iter().enumerate() {
                let cells: Vec<String> = r.iter().map(|(h, c)| cell(c, *h || (header && i == 0))).collect();
                if header && i == 0 { out.push_str(&format!("  table.header({}),\n", cells.join(", "))); } else { out.push_str(&format!("  {},\n", cells.join(", "))); }
            }
            out.push_str(")\n\n");
        }
        "tableOfContents" => {
            if !doc_headings(txn).is_empty() { out.push_str(&format!("#outline(title: [#{}], depth: 4)\n\n", typst_str(toc))); }
        }
        _ => { let body = inline_typst(txn, el, 0); if !body.is_empty() { out.push_str(&body); out.push_str("\n\n"); } }
    }
}

/// Pad-Inhalt als Typst-Markup (Inhalte nur als String-Literale, keine Ausführung von Nutzertext).
// Linien der Daumen wie in web/src/rating.js
const THUMB_UP: &str = r#"<path d="M7 10v12"/><path d="M15 5.88 14 10h5.83a2 2 0 0 1 1.92 2.56l-2.33 8A2 2 0 0 1 17.5 22H4a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2h2.76a2 2 0 0 0 1.79-1.11L12 2a3.13 3.13 0 0 1 3 3.88Z"/>"#;
const THUMB_DOWN: &str = r#"<path d="M17 14V2"/><path d="M9 18.12 10 14H4.17a2 2 0 0 1-1.92-2.56l2.33-8A2 2 0 0 1 6.5 2H20a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2.76a2 2 0 0 0-1.79 1.11L12 22a3.13 3.13 0 0 1-3-3.88Z"/>"#;

pub fn to_typst<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> String { to_typst_mit(txn, frag, VERZEICHNIS) }

pub fn to_typst_mit<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef, toc: &str) -> String {
    let mut out = String::new();
    for c in frag.children(txn) { block_typst(txn, toc, &c, &mut out, 0); }
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
    /// Lauf ist eine Bewertung (Kennung, Stimmen) statt Text
    pub rating: Option<(String, Vec<(String, i8)>)>,
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
    Para { align: Option<String>, indent: u8, runs: Vec<(String, Marks)> },
    Heading { level: u8, indent: u8, runs: Vec<(String, Marks)> },
    List { ordered: bool, items: Vec<Vec<Block>> },
    /// Aufgabenliste: je Eintrag erledigt ja/nein und Inhalt
    TaskList { items: Vec<(bool, Vec<Block>)> },
    /// Tabelle: Zeilen aus Zellen (Kopfzelle?, Inhalt)
    Table { rows: Vec<Vec<(bool, Vec<Block>)>> },
    /// Inhaltsverzeichnis (Inhalt entsteht beim Anzeigen)
    Toc,
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
                        Node::Element(e) => Some(matches!(e.name().to_ascii_lowercase().as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "blockquote" | "pre" | "br" | "hr" | "table" | "tr" | "td" | "th" | "nav")),
                        _ => None,
                    }).unwrap_or(true);
                    if is_block(child.prev_sibling()) || is_block(child.next_sibling()) || runs.is_empty() { continue; }
                }
                if !s.is_empty() { runs.push((s, marks.clone())); }
            }
            Node::Element(e) => {
                let cel = ElementRef::wrap(child).unwrap();
                let name = e.name().to_ascii_lowercase();
                if name == "span" && e.attr("data-type") == Some("rating") {
                    if let Some(id) = e.attr("data-rating").and_then(crate::bausteine::clean_id) {
                        let votes = crate::bausteine::parse_votes_attr(e.attr("data-votes").unwrap_or(""));
                        runs.push((String::new(), Marks { rating: Some((id, votes)), ..marks.clone() }));
                    }
                    continue;
                }
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
                if matches!(name.as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "blockquote" | "pre" | "table" | "nav") {
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
            blocks.push(Block::Para { align: align.clone(), indent: 0, runs: std::mem::take(&mut current) });
            pending_break = true;
        } else {
            current.push((s, m));
            pending_break = false;
        }
    }
    if !current.is_empty() || !pending_break {
        blocks.push(Block::Para { align, indent: 0, runs: current });
    }
}

fn parse_block(el: ElementRef, marks: &Marks) -> Vec<Block> {
    let name = el.value().name().to_ascii_lowercase();
    let indent = el.value().attr("style").and_then(|s| s.split(';').find_map(|d| {
        let (key, value) = d.split_once(':')?;
        (key.trim() == "padding-left").then(|| value.trim().strip_suffix("em")?.trim().parse::<u8>().ok()).flatten()
    })).filter(|n| *n <= 16 && *n % 2 == 0).map_or(0, |n| n / 2);
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
            blocks.push(Block::Heading { level: level.min(4), indent, runs });
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
                    if !runs.is_empty() { item.push(Block::Para { align: None, indent: 0, runs }); }
                    item.extend(inner);
                    if item.is_empty() { item.push(Block::Para { align: None, indent: 0, runs: vec![] }); }
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
                    if !runs.is_empty() { item.push(Block::Para { align: None, indent: 0, runs }); }
                    item.extend(inner);
                    if item.is_empty() { item.push(Block::Para { align: None, indent: 0, runs: vec![] }); }
                    items.push(item);
                }
            }
            blocks.push(Block::List { ordered: name == "ol", items });
        }
        "table" => {
            // Zeilen auch unter thead/tbody/tfoot; Zellinhalt wie ein Listeneintrag
            let mut rows = Vec::new();
            for tr in el.descendants().filter_map(ElementRef::wrap).filter(|e| e.value().name().eq_ignore_ascii_case("tr")) {
                if tr.ancestors().filter_map(ElementRef::wrap).find(|a| a.value().name().eq_ignore_ascii_case("table")).map(|t| t.id()) != Some(el.id()) { continue; }
                let mut row = Vec::new();
                for cell in tr.children().filter_map(ElementRef::wrap) {
                    let cn = cell.value().name().to_ascii_lowercase();
                    if cn != "td" && cn != "th" { continue; }
                    let mut runs = Vec::new();
                    let mut inner = Vec::new();
                    let mut a = None;
                    collect_inline(cell, marks, &mut runs, &mut inner, &mut a);
                    let mut content = Vec::new();
                    flush_runs(&mut runs, &mut content, None);
                    content.extend(inner);
                    content.retain(|b| !matches!(b, Block::Table { .. } | Block::Toc)); // keine Tabellen in Tabellen
                    if content.is_empty() { content.push(Block::Para { align: None, indent: 0, runs: vec![] }); }
                    row.push((cn == "th", content));
                }
                if !row.is_empty() { rows.push(row); }
            }
            if !rows.is_empty() { blocks.push(Block::Table { rows }); }
        }
        "nav" if el.value().attr("data-type") == Some("toc") => blocks.push(Block::Toc),
        "pre" => {
            let text = el.text().collect::<String>();
            blocks.push(Block::Para { align: None, indent: 0, runs: vec![(text, Marks { code: true, ..marks.clone() })] });
        }
        _ => {
            let mut runs = Vec::new();
            let mut a = align;
            collect_inline(el, marks, &mut runs, &mut blocks, &mut a);
            let before = blocks.len();
            flush_runs(&mut runs, &mut blocks, a.clone());
            if name == "p" {
                for block in &mut blocks[before..] {
                    if let Block::Para { indent: n, .. } = block { *n = indent; }
                }
            }
            if blocks.len() == before && name == "p" {
                blocks.push(Block::Para { align: a, indent, runs: vec![] }); // leerer Absatz bleibt erhalten
            }
        }
    }
    blocks
}

fn write_runs(txn: &mut TransactionMut, el: &XmlElementRef, runs: &[(String, Marks)]) {
    if runs.is_empty() { return; }
    // Text kommt in XmlText-Knoten, eine Bewertung als eigenes Element dazwischen
    let mut cur: Option<(XmlTextRef, u32)> = None;
    for (s, m) in runs {
        if let Some((id, votes)) = &m.rating {
            let r: XmlElementRef = el.push_back(txn, XmlElementPrelim::new("rating", Vec::<XmlIn>::new()));
            r.insert_attribute(txn, "id", Any::from(id.as_str()));
            crate::bausteine::write_votes(txn, id, votes);
            cur = None;
            continue;
        }
        let (t, idx) = cur.get_or_insert_with(|| (el.push_back(txn, XmlTextPrelim::new("")), 0));
        t.insert_with_attributes(txn, *idx, s, m.to_attrs());
        *idx += s.encode_utf16().count() as u32;
    }
}

fn write_block(txn: &mut TransactionMut, parent: &XmlElementRef, b: &Block) {
    match b {
        Block::Para { align, indent, runs } => {
            let p: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new("paragraph", Vec::<XmlIn>::new()));
            if let Some(a) = align { p.insert_attribute(txn, "textAlign", Any::from(a.as_str())); }
            if *indent > 0 { p.insert_attribute(txn, "indent", Any::from(*indent as f64)); }
            write_runs(txn, &p, runs);
        }
        Block::Heading { level, indent, runs } => {
            let h: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new("heading", Vec::<XmlIn>::new()));
            h.insert_attribute(txn, "level", Any::from(*level as f64));
            if *indent > 0 { h.insert_attribute(txn, "indent", Any::from(*indent as f64)); }
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
        Block::Table { rows } => {
            let t: XmlElementRef = parent.push_back(txn, XmlElementPrelim::new("table", Vec::<XmlIn>::new()));
            write_rows(txn, &t, rows);
        }
        Block::Toc => { parent.push_back(txn, XmlElementPrelim::new("tableOfContents", Vec::<XmlIn>::new())); }
    }
}

/// Zeilen gleich lang machen (ProseMirror verlangt ein Rechteck) und schreiben.
fn write_rows(txn: &mut TransactionMut, table: &XmlElementRef, rows: &[Vec<(bool, Vec<Block>)>]) {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(1);
    for row in rows {
        let tr: XmlElementRef = table.push_back(txn, XmlElementPrelim::new("tableRow", Vec::<XmlIn>::new()));
        for i in 0..cols {
            let (head, content) = row.get(i).map(|(h, c)| (*h, c.clone())).unwrap_or((false, vec![Block::Para { align: None, indent: 0, runs: vec![] }]));
            let cell: XmlElementRef = tr.push_back(txn, XmlElementPrelim::new(if head { "tableHeader" } else { "tableCell" }, Vec::<XmlIn>::new()));
            for b in &content { write_block(txn, &cell, b); }
        }
    }
}

fn write_block_root(txn: &mut TransactionMut, frag: &XmlFragmentRef, b: &Block) {
    // Wurzel ist ein Fragment: gleiche Logik, anderer Elterntyp
    match b {
        Block::Para { align, indent, runs } => {
            let p: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new("paragraph", Vec::<XmlIn>::new()));
            if let Some(a) = align { p.insert_attribute(txn, "textAlign", Any::from(a.as_str())); }
            if *indent > 0 { p.insert_attribute(txn, "indent", Any::from(*indent as f64)); }
            write_runs(txn, &p, runs);
        }
        Block::Heading { level, indent, runs } => {
            let h: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new("heading", Vec::<XmlIn>::new()));
            h.insert_attribute(txn, "level", Any::from(*level as f64));
            if *indent > 0 { h.insert_attribute(txn, "indent", Any::from(*indent as f64)); }
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
        Block::Table { rows } => {
            let t: XmlElementRef = frag.push_back(txn, XmlElementPrelim::new("table", Vec::<XmlIn>::new()));
            write_rows(txn, &t, rows);
        }
        Block::Toc => { frag.push_back(txn, XmlElementPrelim::new("tableOfContents", Vec::<XmlIn>::new())); }
    }
}

/// Schreibt Blöcke an das Ende des Fragments.
pub fn write_blocks(txn: &mut TransactionMut, frag: &XmlFragmentRef, blocks: &[Block]) {
    for b in blocks { write_block_root(txn, frag, b); }
}

fn clear(txn: &mut TransactionMut, frag: &XmlFragmentRef) {
    let n = frag.len(txn);
    if n > 0 { frag.remove_range(txn, 0, n); }
    crate::bausteine::clear_votes(txn);
}

/// Ersetzt den Inhalt durch reinen Text (Zeilen → Absätze).
pub fn set_text(txn: &mut TransactionMut, frag: &XmlFragmentRef, text: &str) {
    clear(txn, frag);
    let lines: Vec<&str> = text.split('\n').collect();
    let lines = if lines.last() == Some(&"") && lines.len() > 1 { &lines[..lines.len() - 1] } else { &lines[..] };
    for line in lines {
        let runs = if line.is_empty() { vec![] } else { vec![(line.to_string(), Marks::default())] };
        write_block_root(txn, frag, &Block::Para { align: None, indent: 0, runs });
    }
    if lines.is_empty() {
        write_block_root(txn, frag, &Block::Para { align: None, indent: 0, runs: vec![] });
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
            if runs.iter().all(|(s, m)| s.trim().is_empty() && m.rating.is_none()) { blocks.pop(); continue; }
        }
        break;
    }
    if blocks.is_empty() { blocks.push(Block::Para { align: None, indent: 0, runs: vec![] }); }
    for b in &blocks { write_block_root(txn, frag, b); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::{Doc, Map, Transact, WriteTxn};

    /// Pad mit Überschriften, Inhaltsverzeichnis, Tabelle und Bewertung – über den eigenen HTML-Import.
    fn pad() -> Doc {
        let doc = Doc::new();
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let html = r#"<html><body><h1>Planung</h1><nav data-type="toc"></nav>
            <p>Idee A <span data-type="rating" data-rating="r1" data-votes="a1:1,b2:1,c3:-1">x</span> gut</p>
            <h2>Zahlen &amp; Fakten</h2>
            <table><tr><th>Name</th><th>Wert | Einheit</th></tr><tr><td><p>Tee</p></td><td>3</td></tr><tr><td>nur eine</td></tr></table>
            <p><span data-type="rating" data-rating="r2" data-votes=""></span></p></body></html>"#;
        { let mut txn = doc.transact_mut(); set_html(&mut txn, &frag, html); }
        doc
    }

    #[test]
    fn exporte_mit_tabelle_bewertung_und_verzeichnis() {
        let doc = pad();
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let txn = doc.transact();
        let html = to_html(&txn, &frag);
        assert!(html.contains(r#"<h1 id="abschnitt-1">"#) && html.contains(r#"<h2 id="abschnitt-2">"#), "{html}");
        assert!(html.contains(r##"<a href="#abschnitt-2">Zahlen &amp; Fakten</a>"##), "{html}");
        assert!(html.contains("<th><p>Name</p>\n</th>") && html.contains("<td><p>Tee</p>\n</td>"), "{html}");
        assert!(html.contains(r#"data-rating="r1" data-votes="a1:1,b2:1,c3:-1">👍 2 · 👎 1</span>"#), "{html}");
        assert!(html.contains("<p>Idee A ") && html.contains("</span> gut</p>"), "Text um die Bewertung bleibt: {html}");
        let md = to_markdown(&txn, &frag);
        assert!(md.contains("| Name | Wert \\| Einheit |\n| --- | --- |\n| Tee | 3 |\n| nur eine |  |"), "{md}");
        assert!(md.contains("**Inhaltsverzeichnis**\n\n- Planung\n  - Zahlen &amp; Fakten"), "{md}");
        assert!(md.contains("Idee A 👍 2 · 👎 1 gut"), "{md}");
        let text = to_text(&txn, &frag);
        assert!(text.contains("Name | Wert | Einheit\nTee | 3\n"), "{text}");
        let typ = to_typst(&txn, &frag);
        assert!(typ.contains("#outline(") && typ.contains("#table(columns: (1fr,) * 2") && typ.contains("table.header("), "{typ}");
        assert!(typ.contains("image(bytes(") && typ.contains(r##"#"2"#h(5pt)"##) && typ.contains(r##"#"1"]]"##), "Daumen als Bild, 2 hoch, 1 runter: {typ}");
    }

    #[test]
    fn verzeichnis_in_der_sprache_des_exports() {
        let doc = pad();
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let txn = doc.transact();
        let toc = "Table of contents";
        assert!(to_html_mit(&txn, &frag, toc).contains(r#"<div class="toc-title">Table of contents</div>"#));
        assert!(to_markdown_mit(&txn, &frag, toc).contains("**Table of contents**\n\n- Planung"));
        assert!(to_text_mit(&txn, &frag, toc).contains("Table of contents\n"));
        let typ = to_typst_mit(&txn, &frag, toc);
        assert!(typ.contains("#outline(title: [#\"Table of contents\"]"), "{typ}");
        assert!(!typ.contains("Inhaltsverzeichnis") && to_typst(&txn, &frag).contains("Inhaltsverzeichnis"));
    }

    #[test]
    fn eingerueckte_absaetze_in_den_exporten() {
        let doc = Doc::new();
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        {
            let mut txn = doc.transact_mut();
            set_html(&mut txn, &frag, "<h2>Titel</h2><p>Zwei</p><p>Normal</p>");
            let XmlOut::Element(h) = frag.get(&txn, 0).unwrap() else { panic!() };
            h.insert_attribute(&mut txn, "indent", Any::from(1.0));
            let XmlOut::Element(p) = frag.get(&txn, 1).unwrap() else { panic!() };
            p.insert_attribute(&mut txn, "indent", Any::from(2.0));
            p.insert_attribute(&mut txn, "textAlign", Any::from("center"));
        }
        let txn = doc.transact();
        let html = to_html(&txn, &frag);
        assert!(html.contains(r#"<h2 id="abschnitt-1" style="padding-left:2em">"#), "{html}");
        assert!(html.contains(r#"<p style="text-align:center;padding-left:4em">Zwei</p>"#) && html.contains("<p>Normal</p>"), "{html}");
        let text = to_text(&txn, &frag);
        assert!(text.contains("    Titel\n") && text.contains("        Zwei\nNormal"), "{text}");
        let typ = to_typst(&txn, &frag);
        assert!(typ.contains("#pad(left: 2em)[#heading(level: 2)[") && typ.contains("#pad(left: 4em)[#align(center)[#\"Zwei\"]]"), "{typ}");
        let restored = Doc::new();
        let restored_frag = restored.get_or_insert_xml_fragment(FRAGMENT);
        { let mut t = restored.transact_mut(); set_html(&mut t, &restored_frag, &html); }
        let t = restored.transact();
        assert_eq!(to_html(&t, &restored_frag), html, "Einrückung muss den Weg über HTML überstehen");
    }

    #[test]
    fn html_hin_und_zurueck_behaelt_alles() {
        let doc = pad();
        let html = { let frag = doc.get_or_insert_xml_fragment(FRAGMENT); let txn = doc.transact(); to_html(&txn, &frag) };
        let doc2 = Doc::new();
        let frag2 = doc2.get_or_insert_xml_fragment(FRAGMENT);
        { let mut txn = doc2.transact_mut(); txn.get_or_insert_map(crate::bausteine::RATINGS).insert(&mut txn, "alt:x", Any::from(1.0)); set_html(&mut txn, &frag2, &html); }
        let txn = doc2.transact();
        assert_eq!(to_html(&txn, &frag2), html);
        assert!(txn.get_map(crate::bausteine::RATINGS).unwrap().get(&txn, "alt:x").is_none(), "alte Stimmen sind weg");
    }

    #[test]
    fn pdf_mit_tabelle_und_verzeichnis() {
        let doc = pad();
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let body = { let txn = doc.transact(); to_typst(&txn, &frag) };
        let o = crate::typeset::Options { hsd: false, theme_dir: None, logo: None, accent: "#e60028", product: "NextPad", header: "Test", footer: "pad", title: "Test", sprache: crate::texte::Sprache::De };
        let pdf = crate::typeset::pdf(&o, &body).expect("Satz gelingt");
        assert!(pdf.starts_with(b"%PDF"));
    }
}
