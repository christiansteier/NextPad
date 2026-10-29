#![forbid(unsafe_code)]
// Fehlerfall der Handler ist eine fertige axum-Antwort; groß, aber nur auf dem Fehlerpfad und nie gespeichert
#![allow(clippy::result_large_err)]
mod addons;
mod api;
mod bausteine;
mod config;
mod docs;
mod ids;
mod look;
mod live;
mod lti;
mod migrate;
mod net;
mod pdf;
mod qr;
mod retention;
mod render;
mod sperre;
mod texte;
mod store;
mod sync;
mod typeset;
mod web;

use crate::sperre::Sperre;
use axum::{routing::get, Router};

// mimalloc statt Systemallokator: auf Alpine (musl) ist dessen malloc bei vielen Threads deutlich langsamer
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use std::{net::SocketAddr, sync::Arc};

pub struct AppState {
    pub config: config::Config,
    pub store: Arc<store::Store>,
    pub rooms: docs::Rooms,
    /// Neu angelegte freie Pads je Adresse (Zeitstempel ms), für die Begrenzung je Stunde
    pub free_creations: std::sync::Mutex<std::collections::HashMap<String, Vec<i64>>>,
    /// Offene LTI-Anmeldungen und Plattformschlüssel
    pub lti: lti::LtiState,
    /// Live-Meldungen für Addons (Ideensammlung, Quiz, Messung)
    pub live: live::Hub,
    /// Erscheinungsbild aus der Verwaltungsseite
    pub look: look::Looks,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--version") { println!("nextpad {}", env!("CARGO_PKG_VERSION")); return Ok(()); }
    if args.get(1).map(String::as_str) == Some("import-etherpad") {
        // nextpad import-etherpad <export-dir> [--no-history] [--only padID,padID]
        let export = std::path::PathBuf::from(args.get(2).ok_or_else(|| anyhow::anyhow!("Aufruf: nextpad import-etherpad <export-dir> [--no-history] [--only id,id]"))?);
        let data_dir = std::path::PathBuf::from(config::var("DATA").unwrap_or_else(|| "./data".into()));
        std::fs::create_dir_all(&data_dir)?;
        let store = store::Store::open(&config::db_path(&data_dir), &addons::daten())?;
        let with_history = !args.iter().any(|a| a == "--no-history");
        let only: Vec<String> = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default();
        let t0 = std::time::Instant::now();
        let rep = migrate::import(&store, &export, &only, with_history)?;
        println!("Import fertig in {:.1} s: {} Pads, {} Autoren, {} Gruppen, {} Snapshots", t0.elapsed().as_secs_f64(), rep.pads, rep.authors, rep.groups, rep.snapshots);
        println!("Textabweichung gegen inhalt.txt: {} Pads", rep.text_mismatch.len());
        for m in rep.text_mismatch.iter().take(30) { println!("  {m}"); }
        println!("Replay ≠ atext: {} Pads", rep.replay_mismatch.len());
        for m in rep.replay_mismatch.iter().take(30) { println!("  {m}"); }
        println!("Fehler: {}", rep.errors.len());
        for e in rep.errors.iter().take(20) { println!("  {e}"); }
        return Ok(());
    }
    let config = config::Config::from_env()?;
    let _ = web::BASE_PATH.set(config.base_path.clone());
    std::fs::create_dir_all(&config.data_dir)?;
    let store = Arc::new(store::Store::open(&config::db_path(&config.data_dir), &addons::daten())?);
    let look = std::sync::RwLock::new(look::load(&store));
    let state = Arc::new(AppState { rooms: docs::Rooms::new(store.clone()), config, store, free_creations: Default::default(), lti: Default::default(), live: Default::default(), look });
    if args.get(1).map(String::as_str) == Some("compact") {
        // einmal zusammenführen (Frist aus NEXTPAD_HISTORY_DAYS)
        state.store.compact(ids::now_ms() - state.config.history_days * 86_400_000);
        state.store.flush();
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("retention-run") {
        // einmaliger Durchlauf der Aufbewahrung (Modus aus NEXTPAD_RETENTION; dry-run zeigt nur an)
        retention::run_once(&state).await;
        state.rooms.snapshot_all("Aufbewahrung").await;
        state.store.flush();
        return Ok(());
    }

    let app = Router::new()
        .route("/", get(web::index))
        .route("/new", get(web::new_pad))
        .route("/open", get(web::open_pad))
        .route("/me", axum::routing::post(web::rename_me))
        .route("/hilfe", get(web::help))
        .route("/sprache/{kuerzel}", get(web::sprache))
        .route("/verlaengern", get(web::extend_page).post(web::extend_by_token))
        .route("/p/{id}/extend", axum::routing::post(web::extend_owner))
        .route("/p/{id}/delete", axum::routing::post(web::delete_owner))
        .route("/lti/login", get(lti::login).post(lti::login))
        .route("/lti/launch", axum::routing::post(lti::launch))
        .route("/lti/jwks", get(lti::jwks))
        .route("/logout", get(web::logout_confirm).post(lti::logout))
        .route("/static/start.js", get(web::static_start))
        .route("/static/start.css", get(web::static_start_css))
        .route("/api", get(api::version).post(api::version))
        .route("/api/", get(api::version))
        .route("/api/{version}/{func}", get(api::call).post(api::call).layer(axum::extract::DefaultBodyLimit::max(16 << 20)))
        .route("/p/{id}", get(web::page))
        .route("/p/{id}/export/{format}", get(web::export))
        .route("/p/{id}/history", get(web::history))
        .route("/p/{id}/wiederherstellen", axum::routing::post(web::restore))
        .route("/p/{id}/snapshots", get(web::snapshots))
        .route("/p/{id}/snapshots/{ts}", get(web::snapshot_html))
        .route("/ws/{id}", get(web::ws))
        .route("/static/editor.js", get(web::static_js))
        .route("/static/editor.css", get(web::static_css))
        .route("/static/print.js", get(web::static_print))
        .route("/static/icons.woff", get(web::static_icons))
        .route("/static/fond.jpg", get(web::static_fond))
        .route("/static/logo.svg", get(web::static_logo))
        .route("/static/favicon-32.png", get(web::static_favicon))
        .route("/static/apple-touch-icon.png", get(web::static_touch_icon))
        .route("/static/quicksand.ttf", get(web::static_quicksand))
        .route("/static/theme-hsd.css", get(web::static_theme_hsd))
        .route("/theme/logo.png", get(look::logo_file))
        .route("/theme/font.ttf", get(web::theme_font))
        .route("/theme/background.jpg", get(look::background_file))
        .route("/admin/erscheinungsbild", get(look::page).post(look::save).layer(axum::extract::DefaultBodyLimit::max(9 << 20)))
        .route("/admin/erscheinungsbild/vorgabe", axum::routing::post(look::reset))
        .route("/static/admin.js", get(look::static_js))
        .route("/status", get(web::status))
        .merge(addons::routen(&state))
        .fallback(web::not_found)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state.clone());

    // Snapshots nach Ruhephase (3 min) und spätestens stündlich bei Aktivität
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            st.rooms.maintain(180_000, 3_600_000, 600_000).await;
            st.lti.purge();
            // Ratenbegrenzung: leere Einträge entfernen
            st.free_creations.sperren().retain(|_, v| { let now = ids::now_ms(); v.retain(|t| now - *t < 3_600_000); !v.is_empty() });
        }
    });
    // abgelaufene Sessions stündlich entfernen
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            match st.store.purge_expired_sessions(ids::now_ms() / 1000) {
                Ok(n) if n > 0 => tracing::info!("{n} abgelaufene Sessions entfernt"),
                Err(e) => tracing::warn!("Session-Bereinigung: {e}"),
                _ => {}
            }
        }
    });

    // Altbestand des Update-Logs zusammenführen: Feinschritte bleiben für NEXTPAD_HISTORY_DAYS Tage
    let st = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(180)).await;
        loop {
            st.store.compact(ids::now_ms() - st.config.history_days * 86_400_000);
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    });
    // Aufbewahrung freier Pads: kurz nach dem Start, dann täglich
    let st = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        loop {
            retention::run_once(&st).await;
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    });
    tracing::info!("Aufbewahrung freier Pads: {:?}, {} Tage, Ankündigung {} Tage vorher, Mail {}", state.config.retention, state.config.retention_days, state.config.notice_days, if state.config.smtp_url.is_some() { "eingerichtet" } else { "nicht eingerichtet" });
    let st_final = state.clone();
    let bind = state.config.bind;
    for p in &state.config.lti { tracing::info!("LTI-Anmeldung über {} ({})", p.name, p.issuer); }
    tracing::info!("NextPad {} lauscht auf {bind}, Basis-Pfad '{}'", env!("CARGO_PKG_VERSION"), state.config.base_path);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    // geladene Pads beim Beenden sichern
    st_final.rooms.snapshot_all("Beenden").await;
    st_final.store.flush();
    tracing::info!("NextPad beendet");
    Ok(())
}

/// Beenden auf SIGINT (Strg+C) und SIGTERM (systemd, Docker).
async fn shutdown_signal() {
    let ctrl_c = async { let _ = tokio::signal::ctrl_c().await; };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => { s.recv().await; }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("Signal empfangen, beende");
}

#[cfg(test)]
mod tests {
    // Web-Bündel und Server tragen dieselbe Versionsnummer
    #[test]
    fn web_paket_hat_die_version_des_servers() {
        let paket = include_str!("../../web/package.json");
        let zeile = paket.lines().find(|l| l.trim_start().starts_with("\"version\"")).expect("version in web/package.json");
        assert!(zeile.contains(&format!("\"{}\"", env!("CARGO_PKG_VERSION"))), "web/package.json: {zeile}, Cargo.toml: {}", env!("CARGO_PKG_VERSION"));
    }
}
