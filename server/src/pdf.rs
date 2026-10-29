//! Schlichtes PDF aus dem Markdown-Export eines Pads (für den Mailanhang vor dem Löschen).
//!
//! Ohne Fremdbibliothek: Standardschriften Helvetica und Helvetica-Bold mit WinAnsi-Kodierung
//! (deckt Deutsch und Westeuropa ab; andere Zeichen erscheinen als „?“). Überschriften,
//! Absätze und Listen; Zeilenumbruch nach geschätzter Zeichenbreite.

const W: f32 = 595.0; // A4
const H: f32 = 842.0;
const M: f32 = 56.0;

/// Geschätzte Breite eines Zeichens in Helvetica (Anteil der Schriftgröße).
fn width(c: char, bold: bool) -> f32 {
    let w = match c {
        'i' | 'j' | 'l' | '.' | ',' | ';' | ':' | '\'' | '|' | '!' | 'I' => 0.28,
        'f' | 't' | 'r' | ' ' | '(' | ')' | '-' => 0.33,
        'm' | 'w' | 'M' | 'W' | '@' => 0.85,
        'A'..='Z' | 'Ä' | 'Ö' | 'Ü' => 0.68,
        '0'..='9' => 0.56,
        _ => 0.55,
    };
    if bold { w * 1.06 } else { w }
}

/// Zeichen nach WinAnsi; PDF-Sonderzeichen maskiert.
fn encode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.chars() {
        let b: u8 = match c {
            '(' | ')' | '\\' => { out.push(b'\\'); c as u8 }
            ' '..='~' => c as u8,
            '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '€' => 0x80, '‚' => 0x82, '„' => 0x84, '…' => 0x85, '‘' => 0x91, '’' => 0x92,
            '“' => 0x93, '”' => 0x94, '•' => 0x95, '–' => 0x96, '—' => 0x97, '™' => 0x99,
            '\t' => b' ',
            _ => b'?',
        };
        out.push(b);
    }
    out
}

fn wrap(text: &str, size: f32, bold: bool, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0;
    for word in text.split(' ') {
        let ww: f32 = word.chars().map(|c| width(c, bold) * size).sum();
        let sp = width(' ', bold) * size;
        if !cur.is_empty() && cur_w + sp + ww > max {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        // überlange Wörter (Links) hart trennen
        if ww > max {
            for c in word.chars() {
                let cw = width(c, bold) * size;
                if cur_w + cw > max && !cur.is_empty() { lines.push(std::mem::take(&mut cur)); cur_w = 0.0; }
                cur.push(c); cur_w += cw;
            }
            continue;
        }
        if !cur.is_empty() { cur.push(' '); cur_w += sp; }
        cur.push_str(word);
        cur_w += ww;
    }
    if !cur.is_empty() || lines.is_empty() { lines.push(cur); }
    lines
}

/// Markdown-Auszeichnung entfernen, die im PDF nicht dargestellt wird.
fn plain(s: &str) -> String {
    let s = html_escape::decode_html_entities(s).to_string();
    let s = s.replace("**", "").replace("__", "").replace("~~", "");
    let s = s.replace("<u>", "").replace("</u>", "");
    s.chars().filter(|c| !c.is_control()).collect::<String>().replace('\\', "")
}

struct Page { ops: Vec<u8> }

/// PDF aus Titelzeile und Markdown-Text.
pub fn from_markdown(header: &str, md: &str, sp: crate::texte::Sprache) -> Vec<u8> {
    let mut pages: Vec<Page> = vec![Page { ops: Vec::new() }];
    let mut y = H - M;
    let max = W - 2.0 * M;
    let line = |pages: &mut Vec<Page>, y: &mut f32, text: &str, size: f32, bold: bool, indent: f32, before: f32| {
        let lead = size * 1.35;
        *y -= before;
        if *y - lead < M { pages.push(Page { ops: Vec::new() }); *y = H - M; }
        *y -= lead;
        let p = pages.last_mut().expect("Seite");
        p.ops.extend_from_slice(format!("BT /{} {size} Tf {:.1} {:.1} Td (", if bold { "F2" } else { "F1" }, M + indent, *y).as_bytes());
        p.ops.extend(encode(text));
        p.ops.extend_from_slice(b") Tj ET\n");
    };
    for l in wrap(header, 9.0, false, max) { line(&mut pages, &mut y, &l, 9.0, false, 0.0, 0.0); }
    y -= 8.0;
    for raw in md.lines() {
        let t = raw.trim_end();
        if t.trim().is_empty() { y -= 6.0; continue; }
        let lead_spaces = t.len() - t.trim_start().len();
        let t2 = t.trim_start();
        // leerer Listenpunkt („-“ ohne Text)
        let t2 = if t2 == "-" || t2 == "*" { "- " } else { t2 };
        let (text, size, bold, indent, bullet, before) = if let Some(h) = t2.strip_prefix("#### ") { (h, 12.0, true, 0.0, None, 6.0) }
            else if let Some(h) = t2.strip_prefix("### ") { (h, 13.0, true, 0.0, None, 8.0) }
            else if let Some(h) = t2.strip_prefix("## ") { (h, 15.0, true, 0.0, None, 10.0) }
            else if let Some(h) = t2.strip_prefix("# ") { (h, 19.0, true, 0.0, None, 12.0) }
            else if let Some(li) = t2.strip_prefix("- ").or_else(|| t2.strip_prefix("* ")) { (li, 11.0, false, 14.0 + lead_spaces as f32 * 6.0, Some("•".to_string()), 0.0) }
            else if let Some(pos) = t2.find(". ").filter(|p| *p <= 3 && t2[..*p].chars().all(|c| c.is_ascii_digit())) { (&t2[pos + 2..], 11.0, false, 14.0 + lead_spaces as f32 * 6.0, Some(format!("{}.", &t2[..pos])), 0.0) }
            else { (t2, 11.0, false, 0.0, None, 0.0) };
        let text = plain(text);
        let wrapped = wrap(&text, size, bold, max - indent);
        for (i, l) in wrapped.iter().enumerate() {
            if i == 0 {
                if let Some(b) = &bullet {
                    // Aufzählungszeichen auf derselben Zeile vor dem Text
                    let lead = size * 1.35;
                    let mut yy = y - if i == 0 { before } else { 0.0 };
                    if yy - lead < M { pages.push(Page { ops: Vec::new() }); yy = H - M; }
                    let p = pages.last_mut().expect("Seite");
                    p.ops.extend_from_slice(format!("BT /F1 {size} Tf {:.1} {:.1} Td (", M + indent - 12.0, yy - lead).as_bytes());
                    p.ops.extend(encode(b));
                    p.ops.extend_from_slice(b") Tj ET\n");
                    y = yy;
                    line(&mut pages, &mut y, l, size, bold, indent, 0.0);
                    continue;
                }
            }
            line(&mut pages, &mut y, l, size, bold, indent, if i == 0 { before } else { 0.0 });
        }
    }
    // Seitenzahlen
    let n = pages.len();
    for (i, p) in pages.iter_mut().enumerate() {
        p.ops.extend_from_slice(format!("BT /F1 8 Tf {:.1} 30 Td ({} {} {} {n}) Tj ET\n", W - M - 60.0, crate::texte::t(sp, "export.seite"), i + 1, crate::texte::t(sp, "export.von")).as_bytes());
    }
    write(&pages)
}

fn write(pages: &[Page]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n = pages.len();
    // 1 Katalog, 2 Seitenbaum, 3/4 Schriften, dann je Seite: Seite, Inhalt
    let kids: String = (0..n).map(|i| format!("{} 0 R ", 5 + 2 * i)).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    for (i, p) in pages.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>", 6 + 2 * i).into_bytes());
        let mut s = format!("<< /Length {} >>\nstream\n", p.ops.len()).into_bytes();
        s.extend_from_slice(&p.ops);
        s.extend_from_slice(b"\nendstream");
        objs.push(s);
    }
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in &offsets { out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes()); }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// Datum aus Millisekunden seit 1970 als TT.MM.JJJJ (UTC).
pub fn date_de(ms: i64) -> String {
    let (y, m, d) = ymd(ms);
    format!("{d:02}.{m:02}.{y}")
}

/// Datum in der Sprache: deutsch TT.MM.JJJJ, englisch britisch „24 October 2026“ (UTC).
pub fn datum(sp: crate::texte::Sprache, ms: i64) -> String {
    const MONATE: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    match sp {
        crate::texte::Sprache::De => date_de(ms),
        crate::texte::Sprache::En => { let (y, m, d) = ymd(ms); format!("{d} {} {y}", MONATE[(m - 1) as usize]) }
    }
}

/// Jahr, Monat, Tag (UTC) aus Millisekunden seit 1970.
fn ymd(ms: i64) -> (i64, i64, i64) {
    let days = ms.div_euclid(86_400_000);
    // Umrechnung nach Howard Hinnant (civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn datum_de_en() {
        assert_eq!(date_de(0), "01.01.1970"); assert_eq!(date_de(1_792_800_000_000), "24.10.2026");
        assert_eq!(datum(crate::texte::Sprache::En, 1_792_800_000_000), "24 October 2026"); assert_eq!(datum(crate::texte::Sprache::En, 0), "1 January 1970");
    }
    #[test]
    fn pdf_grundform() {
        let p = from_markdown("Kopf", "# Titel\n\nAbsatz mit Ümlaut und (Klammer).\n\n- eins\n- zwei\n1. erstens\n", crate::texte::Sprache::De);
        assert!(p.starts_with(b"%PDF-1.4"));
        assert!(p.ends_with(b"%%EOF\n"));
        let s = String::from_utf8_lossy(&p);
        assert!(s.contains("\\(Klammer\\)"));
    }
}
