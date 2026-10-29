//! QR-Codes zum Mitmachen am Beamer, für Addons.

pub fn block(url: &str, label: &str) -> String {
    let qr = qrcode::QrCode::new(url.as_bytes()).map(|c| c.render::<qrcode::render::svg::Color>().min_dimensions(320, 320).quiet_zone(true).build()).unwrap_or_default();
    let qr = qr.split_once("<svg").map(|(_, r)| format!(r#"<svg role="img" aria-label="QR-Code: {}"{r}"#, html_escape::encode_double_quoted_attribute(label.trim_end_matches(':')))).unwrap_or_default();
    format!(r#"{qr}<p>{}<br><strong>{}</strong></p>"#, html_escape::encode_text(label), html_escape::encode_text(url))
}
