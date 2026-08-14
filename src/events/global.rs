// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Bindings that apply in every context: transport, volume, tabs, help.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, View};
use crate::player::PlayerCmd;
use super::*;

pub(super) fn kitty_delete_album_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=2\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

pub(super) fn kitty_delete_artist_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=3\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

pub(super) fn leaving_album(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::AlbumDetail(_)))
}

pub(super) fn leaving_artist(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::ArtistDetail(_)))
}

/// Bindings that apply everywhere: transport, volume, tabs, help, quit.
///
/// Returns whether the key was consumed. Kept separate from `handle_key` so that
/// panes which capture input — the queue especially — can match their own keys
/// first and then defer here, instead of re-implementing each binding. That
/// duplication had already drifted: the queue re-declared play/pause, shuffle
/// and quit while silently dropping help, the command palette, tab switching,
/// next/previous track and volume.
pub(super) fn handle_global_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('A')
            | KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.toggle_art_fullscreen();
        }
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.help_active = true;
            app.help_scroll = 0;
        }
        KeyCode::Char('/') => {
            app.command.active = true;
            app.command.input.clear();
            app.command.selected = 0;
        }
        KeyCode::Tab => {
            if app.art_fullscreen {
                app.exit_art_fullscreen();
                return true;
            }
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_tab();
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            if app.art_fullscreen {
                app.exit_art_fullscreen();
                return true;
            }
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.prev_tab();
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('t') => app.toggle_queue_visible(),
        KeyCode::Char('n') => app.next_track(),
        KeyCode::Char('p') => app.prev_track(),
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Esc => {
            if app.art_fullscreen {
                app.exit_art_fullscreen();
                return true;
            }
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.go_back();
        }
        KeyCode::Char('+') | KeyCode::Char('=') => { let _ = app.player_tx.send(PlayerCmd::ChangeVolume(5)); },
        KeyCode::Char('-') => { let _ = app.player_tx.send(PlayerCmd::ChangeVolume(-5)); },
        KeyCode::Char('g') => {
            if app.art_fullscreen {
                return true;
            }
            if let Some(track) = get_selected_track(app) {
                app.go_to_artist_from_track(&track);
            } else {
                app.set_status("No track selected".to_string(), crate::app::StatusLevel::Error);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::{mpsc, watch};

    use crate::api::ApiRequest;
    use crate::app::{Preferences, Tab};
    use crate::lastfm::LastfmCmd;
    use crate::mpris::MprisState;

    fn app() -> App {
        let (api_tx, _): (mpsc::UnboundedSender<ApiRequest>, _) = mpsc::unbounded_channel();
        let (player_tx, _): (mpsc::UnboundedSender<PlayerCmd>, _) = mpsc::unbounded_channel();
        let (mpris_tx, _) = watch::channel(MprisState::default());
        let (lastfm_tx, _): (mpsc::UnboundedSender<LastfmCmd>, _) = mpsc::unbounded_channel();
        App::new(api_tx, player_tx, mpris_tx, lastfm_tx, Preferences::default())
    }

    #[test]
    fn shift_a_toggles_fullscreen_without_changing_tabs() {
        let mut app = app();
        app.current_tab = Tab::Albums;
        let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);

        assert!(handle_global_key(&mut app, key));
        assert!(app.art_fullscreen);
        assert_eq!(app.current_tab, Tab::Albums);

        assert!(handle_global_key(&mut app, key));
        assert!(!app.art_fullscreen);
        assert_eq!(app.current_tab, Tab::Albums);
    }

    #[test]
    fn tab_leaves_fullscreen_without_advancing_the_hidden_tab() {
        let mut app = app();
        app.current_tab = Tab::Albums;
        app.art_fullscreen = true;

        assert!(handle_global_key(
            &mut app,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        ));
        assert!(!app.art_fullscreen);
        assert_eq!(app.current_tab, Tab::Albums);
    }
}
