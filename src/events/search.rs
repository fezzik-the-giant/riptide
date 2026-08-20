// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Input for the search box.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::api::ApiRequest;
use crate::app::App;
use crate::search::SearchPane;

pub(super) fn handle_search_input(app: &mut App, key: KeyEvent) {
    match key.code {
        // Esc dismisses the box, matching Esc everywhere else in the app. It
        // used to jump to the previous tab, which was the only way out.
        KeyCode::Esc => {
            app.search.modal_open = false;
        }
        // Tab must keep working while the box is open — it is otherwise
        // swallowed by the catch-all below and the user is stuck.
        KeyCode::Tab => {
            app.search.modal_open = false;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_tab();
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            app.search.modal_open = false;
            app.prev_tab();
        }
        KeyCode::Enter => {
            let query = app.search.query.clone();
            app.search.modal_open = false;
            if !query.is_empty() {
                app.search.loading = true;
                app.search.tracks_awaiting_page2 = true;
                app.search.artists_awaiting_page2 = true;
                app.search.playlists_awaiting_page2 = true;
                app.search.track_sel = 0;
                app.search.artist_sel = 0;
                app.search.playlist_sel = 0;
                app.search.tracks.clear();
                app.search.artists.clear();
                app.search.playlists.clear();
                app.search.reset_viewports();
                app.search.pane = SearchPane::Tracks;
                let _ = app.api_tx.send(ApiRequest::SearchTracks {
                    query: query.clone(),
                });
                let _ = app.api_tx.send(ApiRequest::SearchArtistsMain {
                    query: query.clone(),
                });
                let _ = app.api_tx.send(ApiRequest::SearchPlaylistsMain { query });
            }
        }
        KeyCode::Backspace => {
            app.search.query.pop();
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
        }
        _ => {}
    }
}
