// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Creating playlists and filing tracks into the user's own.

use crate::api::ApiRequest;
use crate::api::models::{AddOutcome, Playlist, Track};

use crate::search::SearchPane;

use super::{
    App, ArtistDetailFocus, NamePrompt, PlaylistDetailFocus, PlaylistPicker, StatusLevel, Tab, View,
};

impl App {
    /// The track a row-level command acts on in the current context, or `None`
    /// where the cursor rests on something that is not a track.
    pub(crate) fn track_under_cursor(&self) -> Option<Track> {
        if self.queue_focused {
            return self.now_playing.queue.get(self.queue_cursor).cloned();
        }
        match self.view_stack.last() {
            Some(View::ArtistDetail(d)) if d.focus == ArtistDetailFocus::Tracks => {
                d.tracks.selected_item().cloned()
            }
            Some(View::PlaylistDetail(d)) if d.focus == PlaylistDetailFocus::Tracks => {
                d.tracks.selected_item().cloned()
            }
            Some(View::AlbumDetail(d)) => d.tracks.selected_item().cloned(),
            Some(_) => None,
            None => match self.current_tab {
                Tab::Favorites => self.favorites.selected_item().cloned(),
                Tab::Search if self.search.pane == SearchPane::Tracks => {
                    self.search.tracks.get(self.search.track_sel).cloned()
                }
                _ => None,
            },
        }
    }

    pub(crate) fn open_playlist_picker(&mut self) {
        let Some(track) = self.track_under_cursor() else {
            self.set_status(
                "Select a track to add it to a playlist".to_string(),
                StatusLevel::Info,
            );
            return;
        };
        let targets = self
            .playlists
            .items
            .iter()
            .filter(|p| self.owned_playlist_ids.contains(&p.uuid))
            .cloned()
            .collect();
        self.playlist_picker = PlaylistPicker {
            active: true,
            track: Some(track),
            targets,
            selected: 0,
        };
    }

    pub(crate) fn pick_playlist(&mut self) {
        let picker = std::mem::take(&mut self.playlist_picker);
        let Some(track) = picker.track else { return };
        match picker
            .selected
            .checked_sub(1)
            .and_then(|i| picker.targets.get(i))
        {
            Some(playlist) => {
                let _ = self.api_tx.send(ApiRequest::AddToPlaylist {
                    playlist: playlist.clone(),
                    track,
                });
            }
            None => self.open_name_prompt(Some(track)),
        }
    }

    pub(crate) fn open_name_prompt(&mut self, then_add: Option<Track>) {
        self.name_prompt = NamePrompt {
            active: true,
            input: String::new(),
            then_add,
        };
    }

    pub(crate) fn submit_name_prompt(&mut self) {
        let name = self.name_prompt.input.trim().to_string();
        if name.is_empty() {
            return;
        }
        let prompt = std::mem::take(&mut self.name_prompt);
        let _ = self.api_tx.send(ApiRequest::CreatePlaylist {
            name,
            then_add: prompt.then_add,
        });
    }

    /// Tidal adds a new playlist to the collection itself, so it joins the
    /// library here as well as the owned set.
    pub(crate) fn on_playlist_created(&mut self, playlist: Playlist, then_add: Option<Track>) {
        self.owned_playlist_ids.insert(playlist.uuid.clone());
        if self.favorite_playlist_ids.insert(playlist.uuid.clone()) {
            self.playlists.items.push(playlist.clone());
            self.playlists.total = self.playlists.total.saturating_add(1);
            self.sort_playlists();
        }
        match then_add {
            Some(track) => {
                let _ = self
                    .api_tx
                    .send(ApiRequest::AddToPlaylist { playlist, track });
            }
            None => self.set_status(format!("Created '{}'", playlist.title), StatusLevel::Info),
        }
    }

    pub(crate) fn on_added_to_playlist(
        &mut self,
        playlist: Playlist,
        track: Track,
        outcome: AddOutcome,
    ) {
        if let AddOutcome::Skipped { reason } = outcome {
            self.set_status(
                format!(
                    "Tidal didn't add '{}' to '{}' ({reason})",
                    track.title, playlist.title
                ),
                StatusLevel::Error,
            );
            return;
        }
        if let Some(p) = self
            .playlists
            .items
            .iter_mut()
            .find(|p| p.uuid == playlist.uuid)
        {
            p.number_of_tracks = Some(p.number_of_tracks.unwrap_or(0) + 1);
        }
        for view in &mut self.view_stack {
            if let View::PlaylistDetail(d) = view {
                // Still paging means a later page will already contain the entry.
                if d.playlist.uuid == playlist.uuid && d.tracks.exhausted {
                    d.tracks.items.push(track.clone());
                    d.tracks.total = d.tracks.total.saturating_add(1);
                    d.tracks.refilter();
                    d.playlist.number_of_tracks = Some(d.tracks.total);
                }
            }
        }
        self.set_status(
            format!("Added '{}' to '{}'", track.title, playlist.title),
            StatusLevel::Info,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::api::ApiRequest;
    use crate::api::models::{AddOutcome, Playlist};
    use crate::app::test_support::{test_app, track};
    use crate::app::{StatusLevel, Tab, View};

    pub(super) fn own(uuid: &str, title: &str) -> Playlist {
        Playlist {
            uuid: uuid.to_string(),
            title: title.to_string(),
            number_of_tracks: Some(2),
            description: None,
            cover: None,
            added_at: None,
            playlist_type: Some("USER".to_string()),
        }
    }

    #[test]
    fn a_created_playlist_joins_the_library_as_yours_then_receives_its_track() {
        let mut t = test_app();
        t.drain_api();
        t.app
            .on_playlist_created(own("new", "Road trip"), Some(track(7)));

        assert!(t.app.owned_playlist_ids.contains("new"));
        assert!(t.app.favorite_playlist_ids.contains("new"));
        assert!(t.app.playlists.items.iter().any(|p| p.uuid == "new"));
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::AddToPlaylist { playlist, track }]
                if playlist.uuid == "new" && track.id == 7
        ));
    }

    #[test]
    fn a_skipped_add_is_reported_as_a_failure_and_changes_nothing() {
        let mut t = test_app();
        t.app.playlists.append_page(vec![own("p", "Mine")], None);
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(7),
            AddOutcome::Skipped {
                reason: "NOT_FOUND".to_string(),
            },
        );
        assert_eq!(t.app.playlists.items[0].number_of_tracks, Some(2));
        let (msg, level, _) = t.app.status.clone().unwrap();
        assert_eq!(level, StatusLevel::Error);
        assert!(msg.contains("NOT_FOUND"), "{msg}");
    }

    #[test]
    fn an_add_appends_to_the_open_detail_once_it_has_finished_loading() {
        let mut t = test_app();
        t.app.open_playlist(own("p", "Mine"));
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks.append_page(vec![track(1)], None);
        }
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(7),
            AddOutcome::Added {
                item_id: "i".to_string(),
            },
        );
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
            panic!("detail closed")
        };
        assert_eq!(
            d.tracks.items.iter().map(|t| t.id).collect::<Vec<_>>(),
            [1, 7]
        );
    }

    /// The next server page already holds the new entry, so appending it here
    /// would show it twice.
    #[test]
    fn an_add_leaves_a_still_paging_detail_to_the_server() {
        let mut t = test_app();
        t.app.open_playlist(own("p", "Mine"));
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks
                .append_page(vec![track(1)], Some("cursor".to_string()));
        }
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(7),
            AddOutcome::Added {
                item_id: "i".to_string(),
            },
        );
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
            panic!("detail closed")
        };
        assert_eq!(d.tracks.items.len(), 1);
    }

    fn on_favorites_with_owned_playlists() -> crate::app::test_support::TestApp {
        let mut t = test_app();
        t.app.current_tab = Tab::Favorites;
        t.app.favorites.append_page(vec![track(7)], None);
        t.app
            .playlists
            .append_page(vec![own("mine", "Mine"), own("theirs", "Theirs")], None);
        t.app.owned_playlist_ids.insert("mine".to_string());
        t.drain_api();
        t
    }

    #[test]
    fn the_picker_offers_new_playlist_then_only_your_own() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.open_playlist_picker();
        let picker = &t.app.playlist_picker;
        assert!(picker.active);
        assert_eq!(picker.track.as_ref().map(|t| t.id), Some(7));
        assert_eq!(
            picker
                .targets
                .iter()
                .map(|p| p.uuid.as_str())
                .collect::<Vec<_>>(),
            ["mine"]
        );
        assert_eq!(picker.rows(), 2);
    }

    #[test]
    fn picking_a_playlist_sends_the_add_and_closes_the_picker() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.open_playlist_picker();
        t.app.playlist_picker.selected = 1;
        t.app.pick_playlist();
        assert!(!t.app.playlist_picker.active);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::AddToPlaylist { playlist, track }]
                if playlist.uuid == "mine" && track.id == 7
        ));
    }

    #[test]
    fn new_playlist_from_the_picker_creates_it_with_the_track_attached() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.open_playlist_picker();
        t.app.pick_playlist();
        assert!(t.app.name_prompt.active);
        t.app.name_prompt.input = "  Road trip ".to_string();
        t.app.submit_name_prompt();
        assert!(!t.app.name_prompt.active);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::CreatePlaylist { name, then_add: Some(track) }]
                if name == "Road trip" && track.id == 7
        ));
    }

    #[test]
    fn a_blank_name_creates_nothing_and_keeps_the_prompt_open() {
        let mut t = test_app();
        t.drain_api();
        t.app.open_name_prompt(None);
        t.app.name_prompt.input = "   ".to_string();
        t.app.submit_name_prompt();
        assert!(t.app.name_prompt.active);
        assert!(t.api_requests().is_empty());
    }

    #[test]
    fn with_no_track_under_the_cursor_the_picker_stays_shut() {
        let mut t = on_favorites_with_owned_playlists();
        t.app.current_tab = Tab::Playlists;
        t.app.open_playlist_picker();
        assert!(!t.app.playlist_picker.active);
        assert!(t.app.status.is_some());
    }

    /// Ownership loads beside Home at startup; its failure must not land in
    /// Home's error slot, which would hide mixes that loaded fine.
    #[test]
    fn a_failed_ownership_load_leaves_home_alone_and_owns_nothing() {
        let mut t = test_app();
        t.app.home_daily_mixes.loading = true;
        t.app
            .handle_api_response(crate::api::ApiResponse::OwnedPlaylistsFailed(
                "HTTP 503".to_string(),
            ));
        assert!(t.app.home_daily_mixes.loading);
        assert_eq!(t.app.home_daily_mixes.error, None);
        assert!(t.app.owned_playlist_ids.is_empty());
        let (msg, level, _) = t.app.status.clone().unwrap();
        assert_eq!(level, StatusLevel::Error);
        assert!(msg.contains("HTTP 503"), "{msg}");
    }

    /// Tidal puts a new playlist into the collection itself, so a library load
    /// still in flight when it is created comes back already holding it.
    #[test]
    fn a_playlist_created_during_the_library_load_is_listed_once() {
        let mut t = test_app();
        t.app.on_playlist_created(own("new", "Road trip"), None);
        t.app
            .handle_api_response(crate::api::ApiResponse::Playlists(
                vec![own("old", "Old"), own("new", "Road trip")],
                2,
            ));
        let uuids: Vec<&str> = t
            .app
            .playlists
            .items
            .iter()
            .map(|p| p.uuid.as_str())
            .collect();
        assert_eq!(
            uuids.iter().filter(|u| **u == "new").count(),
            1,
            "{uuids:?}"
        );
        assert!(uuids.contains(&"old"));
    }
}
