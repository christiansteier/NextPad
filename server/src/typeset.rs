//! PDF-Satz mit Typst: Pad-Inhalt in einer Vorlage, im HSD-Design oder neutral.
//!
//! Typst läuft als Bibliothek im Server, ohne Pakete aus dem Netz und ohne Dateizugriff
//! außer dem Logo. Nutzertext kommt nur als String-Literal in den Satz (render::to_typst).
//! Schriften: Liberation Sans (maßgleich zu Arial, OFL) ist eingebaut; Arial und HSD Sans
//! kommen zur Laufzeit aus NEXTPAD_THEME_DIR, weil sie nicht ins öffentliche Repo dürfen.
use std::sync::OnceLock;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

const LIB_REGULAR: &[u8] = include_bytes!("../static/fonts/LiberationSans-Regular.ttf");
const LIB_BOLD: &[u8] = include_bytes!("../static/fonts/LiberationSans-Bold.ttf");
const LIB_ITALIC: &[u8] = include_bytes!("../static/fonts/LiberationSans-Italic.ttf");
const LIB_BOLDITALIC: &[u8] = include_bytes!("../static/fonts/LiberationSans-BoldItalic.ttf");

struct Fonts { book: LazyHash<FontBook>, fonts: Vec<Font> }

fn library() -> &'static LazyHash<Library> {
    static L: OnceLock<LazyHash<Library>> = OnceLock::new();
    L.get_or_init(|| LazyHash::new(Library::default()))
}

/// Schriften einmal laden: eingebaute plus alle .ttf/.otf aus dem Design-Verzeichnis.
fn fonts(theme_dir: Option<&std::path::Path>) -> &'static Fonts {
    static F: OnceLock<Fonts> = OnceLock::new();
    F.get_or_init(|| {
        let mut fonts: Vec<Font> = Vec::new();
        for data in [LIB_REGULAR, LIB_BOLD, LIB_ITALIC, LIB_BOLDITALIC] { fonts.extend(Font::iter(Bytes::new(data))); }
        if let Some(dir) = theme_dir {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let ok = p.extension().and_then(|x| x.to_str()).map(|x| matches!(x.to_ascii_lowercase().as_str(), "ttf" | "otf")).unwrap_or(false);
                    if ok { if let Ok(b) = std::fs::read(&p) { fonts.extend(Font::iter(Bytes::new(b))); } }
                }
            }
        }
        Fonts { book: LazyHash::new(FontBook::from_fonts(&fonts)), fonts }
    })
}

struct Doc { main: FileId, source: Source, logo: Option<(FileId, Bytes)>, fonts: &'static Fonts }

impl World for Doc {
    fn library(&self) -> &LazyHash<Library> { library() }
    fn book(&self) -> &LazyHash<FontBook> { &self.fonts.book }
    fn main(&self) -> FileId { self.main }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main { Ok(self.source.clone()) } else { Err(FileError::AccessDenied) }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        match &self.logo { Some((lid, b)) if *lid == id => Ok(b.clone()), _ => Err(FileError::AccessDenied) }
    }
    fn font(&self, index: usize) -> Option<Font> { self.fonts.fonts.get(index).cloned() }
    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> { None }
}

fn file_id(path: &str) -> FileId {
    RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).expect("fester Pfad")).intern()
}

pub struct Options<'a> {
    pub hsd: bool,
    pub theme_dir: Option<&'a std::path::Path>,
    /// Kopfzeile rechts, z. B. „NextPad · Stand 24.10.2026“
    pub header: &'a str,
    /// Fußzeile links (Pad-Name)
    pub footer: &'a str,
    pub title: &'a str,
}

/// Vorlage: HSD (Logo, rote Linie, Arial, HSD Sans nur für Ebene 1 ab 20 pt) oder neutral.
fn template(o: &Options, logo: bool, body: &str) -> String {
    use crate::render::typst_str as q;
    let head_left = if o.hsd && logo { "image(\"/logo.png\", height: 11mm)".to_string() } else { "text(size: 13pt, weight: \"bold\")[NextPad]".to_string() };
    let accent = if o.hsd { "rgb(\"#e60028\")" } else { "luma(160)" };
    let h1 = if o.hsd { "#show heading.where(level: 1): set text(font: (\"HSD Sans\", \"Arial\", \"Liberation Sans\"), weight: \"regular\", size: 22pt)" } else { "#show heading.where(level: 1): set text(size: 20pt)" };
    format!(r#"#set document(title: {title}, author: "NextPad")
#set page(paper: "a4", margin: (top: 34mm, bottom: 24mm, left: 25mm, right: 22mm),
  header: [#grid(columns: (1fr, auto), align: (left + horizon, right + horizon), {head_left}, text(size: 8pt, fill: luma(80))[#{header}])
    #v(-1.5mm) #line(length: 100%, stroke: 0.8pt + {accent})],
  footer: context [#set text(size: 8pt, fill: luma(80)); #{footer} #h(1fr) Seite #counter(page).display() von #counter(page).final().first()])
#set text(font: ("Arial", "Liberation Sans"), size: 10.5pt, lang: "de", region: "de", hyphenate: true)
#set par(leading: 0.62em, spacing: 0.9em, justify: false)
#set list(indent: 0.6em, body-indent: 0.5em, marker: ([•], [–], [·]))
#set enum(indent: 0.6em, body-indent: 0.5em)
#show heading: set text(font: ("Arial", "Liberation Sans"), weight: "bold", hyphenate: false)
#show heading: set block(above: 1.3em, below: 0.6em)
{h1}
#show heading.where(level: 2): set text(size: 14pt)
#show heading.where(level: 3): set text(size: 12pt)
#show heading.where(level: 4): set text(size: 11pt)
#show raw: set text(font: ("DejaVu Sans Mono", "Liberation Mono", "Liberation Sans"), size: 9.5pt)
#show raw.where(block: true): block.with(fill: luma(245), inset: 8pt, width: 100%)
#show link: underline

{body}
"#, title = q(o.title), header = q(o.header), footer = q(o.footer))
}

/// Setzt den Pad-Inhalt (Typst-Markup aus render::to_typst) als PDF.
pub fn pdf(o: &Options, body: &str) -> anyhow::Result<Vec<u8>> {
    let fonts = fonts(o.theme_dir);
    let logo = if o.hsd { o.theme_dir.and_then(|d| std::fs::read(d.join("logo.png")).ok()).map(|b| (file_id("/logo.png"), Bytes::new(b))) } else { None };
    let main = file_id("/main.typ");
    let text = template(o, logo.is_some(), body);
    let world = Doc { main, source: Source::new(main, text), logo, fonts };
    let warned = typst::compile::<typst_layout::PagedDocument>(&world);
    let doc = warned.output.map_err(|errs| anyhow::anyhow!("Satz: {}", errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; ")))?;
    let opts = typst_pdf::PdfOptions { creator: typst::foundations::Smart::Custom(Some("NextPad".into())), ..Default::default() };
    let out = typst_pdf::pdf(&doc, &opts);
    // Satz-Cache begrenzen (wie typst-cli nach jedem Lauf), sonst wächst der Speicher im Dauerbetrieb
    typst::comemo::evict(10);
    out.map_err(|errs| anyhow::anyhow!("PDF: {}", errs.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn satz_mit_sonderzeichen() {
        let o = Options { hsd: true, theme_dir: None, header: "NextPad · Stand 24.10.2026", footer: "Pad \"x\" #1", title: "Test" };
        let body = format!("#heading(level: 1)[#{}]\n\n#{} #strong[#{}]\n\n- #{}\n", crate::render::typst_str("Überschrift #1 *x*"), crate::render::typst_str("Text mit \\ und \" und $ und ] und <a>"), crate::render::typst_str("fett"), crate::render::typst_str("Punkt"));
        let p = pdf(&o, &body).expect("PDF");
        assert!(p.starts_with(b"%PDF"));
    }
}
