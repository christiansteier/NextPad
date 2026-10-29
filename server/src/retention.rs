//! Aufbewahrung freier Pads (von der Startseite).
//!
//! Regel:
//! - Pads aus Moodle (mit Gruppe) bleiben, solange Moodle sie nicht löscht. Moodle ruft
//!   deletePad und deleteGroup auf, wenn eine Aktivität oder der ganze Kurs gelöscht wird.
//! - Freie Pads gehören der Person, die sie angelegt hat.
//! - Aktivität heißt: bearbeitet oder geöffnet. Frist: ein Jahr nach der letzten Aktivität,
//!   oder bis zur Verlängerung (`keep_until`), je nachdem, was später liegt.
//! - Besitz ohne Mailadresse (Gäste) oder ohne Besitz (Altbestand, API): Löschen bei Fristablauf.
//! - Besitz mit Mailadresse (über Moodle angemeldet): zwei Wochen vor Ablauf eine Mail mit dem
//!   Inhalt als PDF und einem Link zum Verlängern um ein Jahr. Gelöscht wird frühestens zwei
//!   Wochen nach der Mail. Kann die Mail nicht zugestellt werden, wird nicht gelöscht.
//! - Modus `off` (Standard), `dry-run` (nur protokollieren) oder `on`.
use crate::texte::{t, tf, Sprache};
use crate::{ids, store::RetentionRow, AppState};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Off, DryRun, On }

impl Mode {
    pub fn parse(s: &str) -> Mode { match s { "on" => Mode::On, "dry-run" => Mode::DryRun, _ => Mode::Off } }
}

/// Was die Regel für ein Pad vorsieht.
#[derive(Debug, PartialEq, Eq)]
pub enum Action { Keep, Notify { delete_after: i64 }, Delete, ResetNotice }

/// Ablaufzeitpunkt (ms): letzte Aktivität plus Frist, mindestens bis zur Verlängerung.
pub fn expires(r: &RetentionRow, keep_ms: i64) -> i64 {
    let activity = r.last_edited.max(r.last_seen.unwrap_or(0)).max(r.created);
    (activity + keep_ms).max(r.keep_until.unwrap_or(0))
}

pub fn decide(r: &RetentionRow, now: i64, keep_ms: i64, notice_ms: i64, mail_ok: bool) -> Action {
    let exp = expires(r, keep_ms);
    let has_mail = r.email.as_deref().map(|e| e.contains('@')).unwrap_or(false);
    if let (Some(_), Some(da)) = (r.notice_sent, r.delete_after) {
        // nach der Mail aktiv geworden oder verlängert: Ankündigung zurücknehmen
        if exp > da { return Action::ResetNotice; }
        return if now >= da { Action::Delete } else { Action::Keep };
    }
    if has_mail && mail_ok {
        if now >= exp - notice_ms { return Action::Notify { delete_after: exp.max(now + notice_ms) }; }
        return Action::Keep;
    }
    if has_mail { return Action::Keep; } // ohne Mailversand nie löschen, wenn eine Adresse bekannt ist
    if now >= exp { Action::Delete } else { Action::Keep }
}

/// Ein Durchlauf über alle freien Pads.
pub async fn run_once(state: &Arc<AppState>) {
    let cfg = &state.config;
    if cfg.retention == Mode::Off { return; }
    let keep_ms = cfg.retention_days * 86_400_000;
    let notice_ms = cfg.notice_days * 86_400_000;
    let rows = match state.store.retention_rows() { Ok(r) => r, Err(e) => { tracing::warn!("Aufbewahrung: {e}"); return; } };
    let now = ids::now_ms();
    let mail_ok = cfg.smtp_url.is_some() && cfg.mail_from.is_some() && cfg.public_url.is_some();
    let (mut kept, mut notified, mut deleted, mut reset, mut failed) = (0, 0, 0, 0, 0);
    let dry = cfg.retention == Mode::DryRun;
    for r in rows {
        // geladene Pads mit verbundenen Clients sind gerade aktiv
        if state.rooms.clients_of(&r.id) > 0 { kept += 1; continue; }
        match decide(&r, now, keep_ms, notice_ms, mail_ok) {
            Action::Keep => kept += 1,
            Action::ResetNotice => { reset += 1; if !dry { let _ = state.store.clear_notice(&r.id); } }
            Action::Delete => {
                deleted += 1;
                if dry { tracing::info!("Aufbewahrung (Probelauf): würde Pad {} löschen", r.id); continue; }
                state.rooms.evict(&r.id);
                match state.store.delete_pad(&r.id) { Ok(_) => tracing::info!("Aufbewahrung: Pad {} gelöscht", r.id), Err(e) => tracing::warn!("Aufbewahrung: Löschen {}: {e}", r.id) }
            }
            Action::Notify { delete_after } => {
                if dry { notified += 1; tracing::info!("Aufbewahrung (Probelauf): würde Ankündigung für Pad {} senden", r.id); continue; }
                match notify(state, &r, delete_after).await {
                    Ok(()) => { notified += 1; tracing::info!("Aufbewahrung: Ankündigung für Pad {} gesendet", r.id) }
                    Err(e) => { failed += 1; tracing::warn!("Aufbewahrung: Mail für Pad {} fehlgeschlagen: {e}", r.id) }
                }
            }
        }
    }
    tracing::info!("Aufbewahrung{}: {kept} behalten, {notified} angekündigt, {deleted} gelöscht, {reset} Ankündigungen zurückgenommen, {failed} Mails fehlgeschlagen", if dry { " (Probelauf)" } else { "" });
}

/// Ankündigung mit PDF und Verlängerungslink senden, danach vermerken.
async fn notify(state: &Arc<AppState>, r: &RetentionRow, delete_after: i64) -> anyhow::Result<()> {
    let cfg = &state.config;
    let email = r.email.clone().ok_or_else(|| anyhow::anyhow!("keine Adresse"))?;
    let public = cfg.public_url.clone().ok_or_else(|| anyhow::anyhow!("NEXTPAD_PUBLIC_URL fehlt"))?;
    let md = state.rooms.markdown(&r.id).await?;
    let title = md.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or(&r.id).chars().take(60).collect::<String>();
    let title = html_escape::decode_html_entities(&title).to_string();
    // Addons haben keinen Text für ein PDF; die Mail nennt nur ihren Namen
    let addon = crate::addons::von_pad(state, &r.id);
    let sprache = r.owner_sprache.as_deref().and_then(Sprache::aus_kuerzel);
    // PDF in der gewählten Sprache; ohne Wahl deutsch, wie der erste Teil der Mail
    let pdf = if addon.is_some() { Vec::new() } else { crate::web::pad_pdf(state, &r.id, &r.id, sprache.unwrap_or(Sprache::De)).await? };
    let token = format!("{}{}{}", ids::random16(), ids::random16(), ids::random16());
    let m = Mail {
        titel: &title, addon,
        name: r.owner_name.as_deref().filter(|n| !n.trim().is_empty() && *n != "Gast"),
        pad_link: &format!("{public}/p/{}", crate::web::urlencoding(&r.id)),
        extend_link: &format!("{public}/verlaengern?t={token}"),
        delete_after, site: &cfg.site_name,
    };
    let (subject, text) = m.inhalt(sprache);
    let from = cfg.mail_from.clone().ok_or_else(|| anyhow::anyhow!("NEXTPAD_MAIL_FROM fehlt"))?;
    let url = cfg.smtp_url.clone().ok_or_else(|| anyhow::anyhow!("SMTP fehlt"))?;
    let filename = format!("pad-{}.pdf", r.id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>());
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        use lettre::{message::{header::ContentType, Attachment, MultiPart, SinglePart}, Message, SmtpTransport, Transport};
        let msg = Message::builder()
            .from(from.parse()?)
            .to(email.parse()?)
            .subject(subject)
            .multipart({
                let mp = MultiPart::mixed().singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(text));
                if pdf.is_empty() { mp } else { mp.singlepart(Attachment::new(filename).body(pdf, ContentType::parse("application/pdf")?)) }
            })?;
        let mailer = SmtpTransport::from_url(&url)?.timeout(Some(std::time::Duration::from_secs(30))).build();
        mailer.send(&msg)?;
        Ok(())
    }).await??;
    state.store.set_notice(&r.id, ids::now_ms(), delete_after, &token)?;
    Ok(())
}

/// Inhalt der Mail vor dem Löschen, je Sprache zusammengesetzt.
struct Mail<'a> {
    titel: &'a str,
    addon: Option<&'static crate::addons::Addon>,
    name: Option<&'a str>,
    pad_link: &'a str,
    extend_link: &'a str,
    delete_after: i64,
    site: &'a str,
}

impl Mail<'_> {
    /// Betreff und Text in einer Sprache.
    fn in_sprache(&self, sp: Sprache) -> (String, String) {
        // Addons nennen ihren Namen statt eines Titels aus dem Inhalt
        let titel = self.addon.map_or(self.titel, |a| t(sp, a.name));
        let datum = crate::pdf::datum(sp, self.delete_after);
        let anrede = match self.name { Some(n) => tf(sp, "mail.anrede_name", &[("name", n)]), None => t(sp, "mail.anrede").to_string() };
        let anhang = t(sp, self.addon.map_or("mail.pdf", |a| a.sichern));
        let text = tf(sp, "mail.text", &[("pad_link", self.pad_link), ("anhang", anhang), ("extend_link", self.extend_link), ("site", self.site), ("datum", &datum), ("anrede", &anrede), ("titel", titel)]);
        (tf(sp, "mail.betreff", &[("datum", &datum), ("titel", titel)]), text)
    }

    /// In der gewählten Sprache; ohne Wahl beide in einer Mail, Deutsch zuerst.
    fn inhalt(&self, sprache: Option<Sprache>) -> (String, String) {
        if let Some(sp) = sprache { return self.in_sprache(sp); }
        let (betreff, de) = self.in_sprache(Sprache::De);
        let (subject, en) = self.in_sprache(Sprache::En);
        (format!("{betreff} / {}", subject.trim_start_matches("NextPad: ")), format!("{}\n\n{de}\n-----\n\n{en}", t(Sprache::De, "mail.englisch_unten")))
    }
}

/// Verlängern um die Frist: ab dem späteren Zeitpunkt von jetzt und bisherigem Ablauf,
/// höchstens zwei Fristen in die Zukunft.
pub fn extend(state: &AppState, pad_id: &str) -> anyhow::Result<i64> {
    let keep_ms = state.config.retention_days * 86_400_000;
    let row = state.store.retention_row(pad_id)?.ok_or_else(|| anyhow::anyhow!("kein freies Pad"))?;
    let now = ids::now_ms();
    let until = (expires(&row, keep_ms).max(now) + keep_ms).min(now + 2 * keep_ms);
    state.store.extend(pad_id, until)?;
    Ok(until)
}

#[cfg(test)]
mod tests {
    use super::*;
    const D: i64 = 86_400_000;
    fn row(email: Option<&str>) -> RetentionRow {
        RetentionRow { id: "x".into(), created: 0, last_edited: 0, last_seen: None, keep_until: None, notice_sent: None, delete_after: None, owner_id: Some("a.1".into()), email: email.map(str::to_string), owner_name: None, owner_sprache: None }
    }
    #[test]
    fn gast_nach_einem_jahr() {
        let r = row(None);
        assert_eq!(decide(&r, 364 * D, 365 * D, 14 * D, true), Action::Keep);
        assert_eq!(decide(&r, 365 * D, 365 * D, 14 * D, true), Action::Delete);
    }
    #[test]
    fn mit_mail_erst_ankuendigen() {
        let mut r = row(Some("a@b.de"));
        assert_eq!(decide(&r, 340 * D, 365 * D, 14 * D, true), Action::Keep);
        assert_eq!(decide(&r, 351 * D, 365 * D, 14 * D, true), Action::Notify { delete_after: 365 * D });
        // spät entdeckt: trotzdem zwei Wochen nach der Mail
        assert_eq!(decide(&r, 400 * D, 365 * D, 14 * D, true), Action::Notify { delete_after: 414 * D });
        r.notice_sent = Some(351 * D); r.delete_after = Some(365 * D);
        assert_eq!(decide(&r, 360 * D, 365 * D, 14 * D, true), Action::Keep);
        assert_eq!(decide(&r, 365 * D, 365 * D, 14 * D, true), Action::Delete);
        // nach der Mail geöffnet: Ankündigung zurück
        r.last_seen = Some(355 * D);
        assert_eq!(decide(&r, 360 * D, 365 * D, 14 * D, true), Action::ResetNotice);
    }
    #[test]
    fn ohne_mailversand_nie_loeschen() {
        let r = row(Some("a@b.de"));
        assert_eq!(decide(&r, 900 * D, 365 * D, 14 * D, false), Action::Keep);
    }
    fn mail(addon: Option<&'static crate::addons::Addon>) -> Mail<'static> {
        Mail { titel: "Plan {datum}", addon, name: Some("Kim"), pad_link: "https://x/p/a", extend_link: "https://x/verlaengern?t=1", delete_after: 1_792_800_000_000, site: "HSD" }
    }
    #[test]
    fn mail_in_einer_sprache() {
        let (b, t) = mail(None).inhalt(Some(Sprache::En));
        assert_eq!(b, "NextPad: pad “Plan {datum}” will be deleted on 24 October 2026");
        assert!(t.starts_with("Hello Kim,\n\nthe pad “Plan {datum}”") && t.contains("attached as a PDF") && t.contains("https://x/verlaengern?t=1") && !t.contains("Guten Tag"), "{t}");
        let (b, t) = mail(Some(&crate::addons::quiz::ADDON)).inhalt(Some(Sprache::De));
        assert_eq!(b, "NextPad: Pad „Quiz-Raster“ wird am 24.10.2026 gelöscht");
        assert!(t.contains("im Quiz als Markdown oder Text sichern") && !t.contains("PDF"), "{t}");
    }
    #[test]
    fn mail_ohne_wahl_in_beiden_sprachen() {
        let (b, t) = mail(None).inhalt(None);
        assert_eq!(b, "NextPad: Pad „Plan {datum}“ wird am 24.10.2026 gelöscht / pad “Plan {datum}” will be deleted on 24 October 2026");
        let (de, en) = (t.find("Guten Tag Kim,").unwrap(), t.find("Hello Kim,").unwrap());
        assert!(t.starts_with("English version below.") && de < en, "{t}");
    }
    #[test]
    fn verlaengert() {
        let mut r = row(None);
        r.keep_until = Some(700 * D);
        assert_eq!(decide(&r, 600 * D, 365 * D, 14 * D, true), Action::Keep);
        assert_eq!(decide(&r, 700 * D, 365 * D, 14 * D, true), Action::Delete);
    }
}
