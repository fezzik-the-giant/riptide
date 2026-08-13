// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Bindings that apply in every context: transport, volume, tabs, help.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::app::{App, View};
use crate::player::PlayerCmd;

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
        KeyCode::Char('A') | KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::SHIFT) => {
            app.toggle_art_fullscreen();
        }
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.help_active = true;
            app.help_scroll = 0;
        }
        // `:` for the command line, `/` for finding things within the current
        // view — the vim split. `/` used to open the palette.
        KeyCode::Char(':') => {
            app.command.active = true;
            app.command.input.clear();
            app.command.selected = 0;
        }
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc if app.art_fullscreen => {
            app.exit_art_fullscreen();
        }
        // Not while the queue has focus: the filter narrows the tab's list, which
        // is not what the user is looking at. Not in fullscreen art either — the
        // filter overlay is hidden there, so it would silently swallow Tab/Esc
        // (which should exit fullscreen) into an invisible filter box.
        KeyCode::Char('/') if app.filterable_tab() && !app.queue_focused && !app.art_fullscreen => {
            app.filter_active = true;
        }
        KeyCode::Tab => {
            if leaving_album(app) {
                kitty_delete_album_art();
            }
            if leaving_artist(app) {
                kitty_delete_artist_art();
            }
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_tab();
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            if leaving_album(app) {
                kitty_delete_album_art();
            }
            if leaving_artist(app) {
                kitty_delete_artist_art();
            }
            app.prev_tab();
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('t') => app.toggle_queue_visible(),
        KeyCode::Char('n') => app.next_track(),
        KeyCode::Char('p') => app.prev_track(),
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Char('u') => app.undo_last_removal(),
        KeyCode::Esc => {
            // A filter left applied after the box closed would otherwise have no
            // quick way out; clearing it takes priority over navigating back.
            if app.filterable_tab() && !app.active_filter().is_empty() {
                app.clear_active_filter();
                return true;
            }
            if leaving_album(app) {
                kitty_delete_album_art();
            }
            if leaving_artist(app) {
                kitty_delete_artist_art();
            }
            app.go_back();
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            let _ = app.player_tx.send(PlayerCmd::ChangeVolume(5));
        }
        KeyCode::Char('-') => {
            let _ = app.player_tx.send(PlayerCmd::ChangeVolume(-5));
        }
        KeyCode::Char('g') => {
            if app.art_fullscreen {
                return true;
            }
            if let Some(track) = get_selected_track(app) {
                app.go_to_artist_from_track(&track);
            } else {
                app.set_status(
                    "No track selected".to_string(),
                    crate::app::StatusLevel::Error,
                );
            }
        }
        KeyCode::Char('U') => {
            use crate::app::UpdateStatus;
            if app.update.status == UpdateStatus::Done {
                // An install already succeeded this session; re-open it rather
                // than falling through to "up to date", which would name the
                // version the user is still running.
                app.open_update_dialog_in_state(UpdateStatus::Done);
            } else if app.update.check_done {
                if app.update.available.is_some() {
                    app.open_update_dialog_in_state(UpdateStatus::Confirming);
                } else if app.update.check_error.is_some() {
                    if let Some(err) = app.update.check_error.clone() {
                        app.update.error = Some(err);
                    }
                    app.open_update_dialog_in_state(UpdateStatus::Failed);
                } else {
                    app.open_update_dialog_in_state(UpdateStatus::UpToDate);
                }
            } else if app.update.self_updatable == Some(false) {
                app.set_status(
                    "Updates are handled by your package manager".to_string(),
                    crate::app::StatusLevel::Info,
                );
            } else {
                // Either the actor has not resolved the install method yet or
                // the first check is still in flight; both are momentary and
                // neither justifies claiming an answer.
                app.set_status(
                    "Checking for updates…".to_string(),
                    crate::app::StatusLevel::Info,
                );
            }
        }
        // Alias for help on top-level tabs that don't use 'h' for navigation.
        // Guard to avoid hijacking 'h' (left) in detail views and Home/Search;
        // text boxes and queue focus never reach here (see handle_key dispatch).
        KeyCode::Char('h') | KeyCode::Char('H') => {
            let can_open_help = app.view_stack.is_empty()
                && !app.art_fullscreen
                && app.current_tab != Tab::Home
                && app.current_tab != Tab::Search;
            if can_open_help {
                app.help_active = true;
                app.help_scroll = 0;
                app.help_query.clear();
            } else {
                return false;
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Tab;
    use crate::app::test_support::test_app;

    fn press_u(app: &mut crate::app::App) {
        assert!(handle_global_key(
            app,
            KeyEvent::new(KeyCode::Char('U'), KeyModifiers::SHIFT)
        ));
    }

    #[test]
    fn u_does_not_claim_a_package_manager_before_the_actor_reports() {
        let mut t = test_app();
        // self_updatable is None for the first seconds of every session, while
        // the actor resolves the install method off the first frame.
        press_u(&mut t.app);
        let (msg, _, _) = t.app.status.clone().expect("U must give some feedback");
        assert!(
            msg.contains("Checking"),
            "an unresolved install method must not be reported as one: {msg}"
        );
        assert!(!t.app.update.active);
    }

    #[test]
    fn shift_a_toggles_fullscreen_without_changing_tabs() {
        let mut t = test_app();
        t.app.current_tab = Tab::Albums;
        t.app.queue_focused = true;
        let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);

        assert!(handle_global_key(&mut t.app, key));
        assert!(t.app.art_fullscreen);
        assert_eq!(t.app.current_tab, Tab::Albums);
        assert!(t.app.queue_focused);

        assert!(handle_global_key(&mut t.app, key));
        assert!(!t.app.art_fullscreen);
        assert_eq!(t.app.current_tab, Tab::Albums);
        assert!(t.app.queue_focused);
    }

    #[test]
    fn tab_leaves_fullscreen_without_advancing_the_hidden_tab() {
        let mut t = test_app();
        t.app.current_tab = Tab::Albums;
        t.app.art_fullscreen = true;

        assert!(handle_global_key(
            &mut t.app,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        ));
        assert!(!t.app.art_fullscreen);
        assert_eq!(t.app.current_tab, Tab::Albums);
    }

    #[test]
    fn slash_does_not_open_the_hidden_filter_in_fullscreen_art() {
        let mut t = test_app();
        t.app.current_tab = Tab::Albums;
        t.app.art_fullscreen = true;
        assert!(t.app.filterable_tab());

        handle_global_key(
            &mut t.app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        );

        assert!(!t.app.filter_active);
    }
}
