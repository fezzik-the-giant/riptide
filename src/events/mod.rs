// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Input handling and the main event loop.
//!
//! [`run_app`] drives the draw/poll cycle. Key dispatch in `handle_key` takes
//! the text boxes first, then rewrites `j`/`k` into arrow keys, then works
//! through the remaining contexts that capture input — help, the queue, the
//! pickers — before falling through to the global bindings and list navigation.

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::ApiResponse;
use crate::app::{App, Tab};
use crate::mpris::MprisCmd;
use crate::player::PlayerEvent;

mod filter;
mod global;
mod navigation;
mod overlays;
mod queue;
mod search;

use filter::*;
use global::*;
use navigation::*;
use overlays::*;
use queue::*;
use search::*;

// Watches for SIGINT, SIGTERM and SIGHUP and reports them through the flag.
// Closing the terminal window used to kill the process outright, losing the
// session's preference changes (sorts, volume, queue visibility) because those
// are only written on a clean exit.
pub fn spawn_signal_watcher(shutdown: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // A single-signal wait needs no worker pool; current_thread suffices.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            // The flag must only rise after a real signal.
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                // All three must register or none are used: a partial set
                // leaves tokio's process-wide handler installed with no
                // receiver alive, silently swallowing that signal.
                let (mut term, mut hup, mut int) = match (
                    signal(SignalKind::terminate()),
                    signal(SignalKind::hangup()),
                    signal(SignalKind::interrupt()),
                ) {
                    (Ok(t), Ok(h), Ok(i)) => (t, h, i),
                    (_, _, Err(e)) | (Ok(_), Err(e), _) | (Err(e), _, _) => {
                        tracing::warn!(
                            "signal handlers unavailable ({e}); quitting without graceful shutdown"
                        );
                        return;
                    }
                };
                tokio::select! {
                    _ = term.recv() => {}
                    _ = hup.recv() => {}
                    _ = int.recv() => {}
                }
                shutdown.store(true, Ordering::Relaxed);
                // Tokio's handler stays installed for the process's lifetime,
                // so an unhandled repeat would be swallowed and could no longer
                // force past a stalled save or worker join.
                while int.recv().await.is_some() {
                    tracing::warn!("repeated signal during shutdown; exiting immediately");
                    std::process::exit(143);
                }
            }
            #[cfg(not(unix))]
            match tokio::signal::ctrl_c().await {
                Ok(()) => shutdown.store(true, Ordering::Relaxed),
                Err(e) => tracing::warn!(
                    "ctrl-c handler unavailable ({e}); quitting without graceful shutdown"
                ),
            }
        });
    })
}

pub fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut api_rx: mpsc::UnboundedReceiver<ApiResponse>,
    mut player_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    mut mpris_rx: mpsc::UnboundedReceiver<MprisCmd>,
    lastfm_evt_tx: mpsc::UnboundedSender<PlayerEvent>,
    signal_shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| crate::ui::draw(f, app))?;

        // Drain API responses
        while let Ok(resp) = api_rx.try_recv() {
            app.handle_api_response(resp);
        }

        // Drain player events and forward to Last.fm
        while let Ok(evt) = player_rx.try_recv() {
            let _ = lastfm_evt_tx.send(evt.clone());
            app.handle_player_event(evt);
        }

        // Drain MPRIS control commands
        while let Ok(cmd) = mpris_rx.try_recv() {
            match cmd {
                MprisCmd::Next => app.next_track(),
                MprisCmd::Previous => app.prev_track(),
                MprisCmd::Play => app.mpris_play(),
                MprisCmd::Pause => app.set_paused(true),
                MprisCmd::PlayPause => app.mpris_play_pause(),
                MprisCmd::Stop => app.stop_playback(),
                MprisCmd::Quit => app.should_quit = true,
                MprisCmd::SetVolume(v) => {
                    app.set_volume_percent((v.clamp(0.0, 1.0) * 100.0).round() as u8)
                }
                MprisCmd::SetShuffle(on) => app.set_shuffle(on),
                MprisCmd::Seek(offset_us) => app.seek_by_us(offset_us),
                MprisCmd::SetPosition(track_id, position_us) => {
                    app.set_position_us(track_id, position_us)
                }
            }
        }

        app.tick();

        if signal_shutdown.load(Ordering::Relaxed) {
            app.should_quit = true;
        }

        if app.should_quit {
            break;
        }

        // Poll for key events with a short timeout to keep animations smooth
        // Drain all pending key events and only process the last one to avoid lag from key repeat
        let mut last_key_event: Option<KeyEvent> = None;
        while event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                last_key_event = Some(key);
            }
        }
        if let Some(key) = last_key_event {
            handle_key(app, key);
        }

        // Small delay to keep animations smooth
        if !event::poll(Duration::from_millis(16))? {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

/// `j`/`k` stand in for `Down`/`Up`. `h`/`l` already move between panes, so the
/// same hand should move within one.
///
/// Rewriting the event once, here, reaches every list — the tabs, the detail
/// views, the queue and the overlays — where adding a `Char('j')` arm beside
/// each of the fifteen `KeyCode::Down` arms would leave the two spellings free
/// to drift apart. Modified presses are left alone: the queue gives `Ctrl+Up`
/// and `Ctrl+Down` a meaning of their own, and terminals send `Ctrl+J` as Enter.
fn vim_arrows(mut key: KeyEvent) -> KeyEvent {
    if !key.modifiers.is_empty() {
        return key;
    }
    key.code = match key.code {
        KeyCode::Char('j') => KeyCode::Down,
        KeyCode::Char('k') => KeyCode::Up,
        code => code,
    };
    key
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // Any keystroke means the user is working the list, so restart the marquee
    // and let them read the row they just landed on from its beginning.
    app.marquee_epoch = std::time::Instant::now();

    // A text box outranks every other context, because to it a keystroke is a
    // character and nothing else may claim it first. The queue used to be
    // checked ahead of the command palette and swallowed everything typed into
    // a palette opened from it, which made `:` in there a dead end.
    if app.command.active {
        handle_command_input(app, key);
        return;
    }

    if app.filter_active {
        handle_filter_input(app, key);
        return;
    }

    // The search box captures all keys while open, regardless of current tab.
    if app.search.modal_open {
        handle_search_input(app, key);
        return;
    }

    // Past the text boxes, letters are commands again.
    let key = vim_arrows(key);

    if app.help_active {
        handle_help_input(app, key);
        return;
    }

    // Fullscreen art is a presentation layer over the active view. Only global
    // controls apply while it is open, so list navigation cannot mutate the
    // view hidden beneath it. It also outranks queue focus so Esc dismisses
    // art instead of the queue.
    if app.art_fullscreen {
        handle_global_key(app, key);
        return;
    }

    if app.queue_focused {
        handle_queue_input(app, key);
        return;
    }

    if app.sort_palette.active {
        handle_sort_palette_input(app, key);
        return;
    }

    if app.artist_selection.active {
        handle_artist_selection_input(app, key);
        return;
    }

    // Open search modal when on Search tab
    if app.current_tab == Tab::Search {
        if key.code == KeyCode::Char('/') {
            app.search.modal_open = true;
            app.search.query.clear();
            return;
        }
    }

    if !handle_global_key(app, key) {
        handle_navigation(app, key);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Artist;
    use crate::app::test_support::{TestApp, test_app};
    use crossterm::event::KeyModifiers;

    fn app_on_artists_tab() -> TestApp {
        let mut t = test_app();
        t.app.current_tab = Tab::Artists;
        t.app.artists.append_page(
            (0..3)
                .map(|id| Artist {
                    id,
                    name: format!("Artist {id}"),
                    added_at: None,
                })
                .collect(),
            None,
        );
        t
    }

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn j_and_k_move_the_selection() {
        let mut t = app_on_artists_tab();

        handle_key(&mut t.app, press('j'));
        assert_eq!(t.app.artists.selected, 1);

        handle_key(&mut t.app, press('k'));
        assert_eq!(t.app.artists.selected, 0);
    }

    /// The queue used to be checked before the palette and ate everything typed
    /// into one opened from it.
    #[test]
    fn the_command_palette_outranks_the_focused_queue() {
        let mut t = app_on_artists_tab();
        t.app.queue_focused = true;

        handle_key(&mut t.app, press(':'));
        handle_key(&mut t.app, press('c'));

        assert!(t.app.command.active);
        assert_eq!(t.app.command.input, "c");
    }

    /// The half that is easy to break: inside a text box they are letters again.
    #[test]
    fn the_filter_box_reads_j_and_k_as_letters() {
        let mut t = app_on_artists_tab();
        t.app.filter_active = true;

        handle_key(&mut t.app, press('j'));
        handle_key(&mut t.app, press('k'));

        assert_eq!(t.app.active_filter(), "jk");
        assert_eq!(t.app.artists.selected, 0);
    }

    #[test]
    fn fullscreen_art_preempts_queue_focus_for_escape_and_navigation() {
        let mut t = test_app();
        t.app.art_fullscreen = true;
        t.app.queue_focused = true;

        handle_key(
            &mut t.app,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        assert!(t.app.art_fullscreen);
        assert!(t.app.status.is_none());
        assert!(t.app.view_stack.is_empty());

        handle_key(&mut t.app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!t.app.art_fullscreen);
        assert!(t.app.queue_focused);
    }
}
