// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use tokio::sync::mpsc;

mod api;
mod app;
mod events;
mod lastfm;
mod manifest;
mod mpris;
mod player;
mod playlist;
mod search;
mod ui;

use api::ApiWorker;
use app::App;
use lastfm::auth as lastfm_auth_module;
use mpris::MprisServer;
use player::PlayerWorker;

fn lastfm_auth() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(lastfm_auth_module::authenticate())?;
    Ok(())
}

fn setup_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            std::io::stderr(),
            LeaveAlternateScreen,
            crossterm::cursor::Show,
        );
        original(info);
    }));
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.iter().any(|a| a == "--lastfm-auth") {
        return lastfm_auth();
    }

    setup_panic_hook();

    // Initialize logging to file
    if let Ok(home) = std::env::var("HOME") {
        let log_dir = std::path::PathBuf::from(home).join(".local/share/riptide");
        let _ = std::fs::create_dir_all(&log_dir);
        let file_appender = tracing_appender::rolling::daily(&log_dir, "riptide.log");

        let log_level = std::env::var("RIPTIDE_LOG_LEVEL")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| "warn".to_string());
        // A bare level applies to riptide only. Left global it also turns on
        // hyper's per-connection chatter, which drowned real events: connection
        // pooling alone was a quarter of the lines in a user's debug log. A full
        // directive ("riptide=debug,hyper=info") is still honoured as written.
        let directive = if log_level.contains('=') {
            log_level.clone()
        } else {
            format!("riptide={log_level}")
        };
        let env_filter = tracing_subscriber::EnvFilter::new(&directive);

        let _ = tracing_subscriber::fmt()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_env_filter(env_filter)
            .with_thread_ids(false)
            .with_target(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .init();
    }

    tracing::info!("riptide {} starting", env!("CARGO_PKG_VERSION"));
    let mut config = api::auth::load_config()?;
    api::auth::ensure_auth(&mut config)?;
    tracing::info!(
        "authenticated as user {}",
        config.user_id.unwrap_or_default()
    );

    // Channels: TUI → ApiWorker and TUI → PlayerWorker
    let (api_req_tx, api_req_rx) = mpsc::unbounded_channel();
    let (api_resp_tx, api_resp_rx) = mpsc::unbounded_channel();
    let (player_cmd_tx, player_cmd_rx) = mpsc::unbounded_channel::<crate::player::PlayerCmd>();
    let (player_evt_tx, player_evt_rx) = mpsc::unbounded_channel();
    let (player_evt_lastfm_tx, player_evt_lastfm_rx) = mpsc::unbounded_channel();

    // Channels for MPRIS: TUI → MPRIS server (state updates) and MPRIS → TUI (control commands)
    let (mpris_state_tx, mpris_state_rx) =
        tokio::sync::watch::channel(mpris::MprisState::default());
    let (mpris_cmd_tx, mpris_cmd_rx) = mpsc::unbounded_channel::<mpris::MprisCmd>();

    // Channels for Last.fm worker: TUI → Last.fm and Last.fm → TUI
    let (lastfm_cmd_tx, lastfm_cmd_rx) = mpsc::unbounded_channel::<lastfm::LastfmCmd>();
    let (_lastfm_evt_tx, _lastfm_evt_rx) = mpsc::unbounded_channel::<lastfm::LastfmEvent>();

    // Spawn async workers on a dedicated Tokio thread.
    // We keep the handle so we can join it on exit and let PlayerWorker kill mpv cleanly.
    let worker_config = config.clone();
    let worker_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let api_worker = ApiWorker::new(worker_config.clone(), api_req_rx, api_resp_tx);
            let player_worker = PlayerWorker::new(player_cmd_rx, player_evt_tx);
            let mpris_server = MprisServer::new(mpris_state_rx, mpris_cmd_tx);
            let lastfm_worker = lastfm::worker::LastfmWorker::new(
                worker_config.lastfm,
                lastfm_cmd_rx,
                player_evt_lastfm_rx,
                _lastfm_evt_tx,
            );
            tokio::spawn(manifest::run_server());
            tokio::join!(
                api_worker.run(),
                player_worker.run(),
                mpris_server.run(),
                lastfm_worker.run()
            );
        });
    });

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Build app state and run
    let mut app = App::new(
        api_req_tx,
        player_cmd_tx,
        mpris_state_tx,
        lastfm_cmd_tx,
        config.prefs.clone(),
    );
    let result = events::run_app(
        &mut terminal,
        &mut app,
        api_resp_rx,
        player_evt_rx,
        mpris_cmd_rx,
        player_evt_lastfm_tx,
    );

    // Restore terminal unconditionally
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Persist UI choices on the way out — one write, rather than rewriting a
    // file containing OAuth tokens on every volume keypress. A crash loses the
    // session's preference changes, which is an acceptable trade for not
    // touching the credential file continuously.
    config.prefs = app.preferences();
    if let Err(e) = api::auth::save_config(&config) {
        tracing::error!("Failed to save preferences: {e}");
    }

    // Dropping app closes the command channels, which causes both workers to exit
    // their loops. Joining ensures PlayerWorker reaches child.kill() before we return.
    drop(app);
    let _ = worker_thread.join();

    if result.is_ok() {
        tracing::info!("Application shutdown complete");
    } else {
        tracing::error!("Application error: {:?}", result);
    }

    result
}
