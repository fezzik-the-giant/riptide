// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Input for the queue panel.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::app::App;

pub(super) fn handle_queue_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.unfocus_queue();
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_queue_track_up();
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_queue_track_down();
        }
        KeyCode::Up => {
            if app.queue_cursor > 0 {
                app.queue_cursor -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.now_playing.queue.len();
            if len > 0 && app.queue_cursor + 1 < len {
                app.queue_cursor += 1;
            }
        }
        KeyCode::PageUp => app.queue_page_up(),
        KeyCode::PageDown => app.queue_page_down(),
        KeyCode::Enter => {
            let cursor = app.queue_cursor;
            app.play_from_queue(cursor);
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let cursor = app.queue_cursor;
            app.remove_from_queue(cursor);
        }
        KeyCode::Char('f') => {
            if let Some(track) = app.now_playing.queue.get(app.queue_cursor).cloned() {
                app.toggle_favorite_track(&track);
            }
        }
        KeyCode::Char('c') => {
            if let Some(url) = app
                .now_playing
                .queue
                .get(app.queue_cursor)
                .map(|t| t.share_url())
            {
                app.copy_url(url);
            }
        }
        KeyCode::Char('C') => {
            if let Some(url) = app
                .now_playing
                .queue
                .get(app.queue_cursor)
                .map(|t| t.album.share_url())
            {
                app.copy_url(url);
            }
        }
        // Targets the cursor rather than the playing track: get_selected_track()
        // (used by the global binding) has no notion of the queue cursor and
        // would jump to the wrong artist while the queue is focused.
        KeyCode::Char('g') => {
            if let Some(track) = app.now_playing.queue.get(app.queue_cursor).cloned() {
                app.go_to_artist_from_track(&track);
            } else {
                app.set_status(
                    "No track selected".to_string(),
                    crate::app::StatusLevel::Error,
                );
            }
        }
        // Anything the queue doesn't claim falls through to the global bindings
        // so transport, volume, tabs and help keep working in here.
        _ => {
            handle_global_key(app, key);
        }
    }
}
