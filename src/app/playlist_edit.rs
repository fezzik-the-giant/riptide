// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Creating, renaming and deleting playlists, and filing tracks into the user's own.

use crate::api::ApiRequest;
use crate::api::models::{AddOutcome, Playlist, Track};

use crate::search::SearchPane;

use super::{
    App, ArtistDetailFocus, NamePrompt, NameTarget, PlaylistDetailFocus, PlaylistMove,
    PlaylistPicker, StatefulList, StatusLevel, Tab, View,
};

fn entry_position(tracks: &StatefulList<Track>, item_id: &str) -> Option<usize> {
    tracks
        .items
        .iter()
        .position(|t| t.playlist_item_id.as_deref() == Some(item_id))
}

fn entry_after(tracks: &StatefulList<Track>, at: usize) -> Option<String> {
    tracks
        .items
        .get(at + 1)
        .and_then(|t| t.playlist_item_id.clone())
}

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
            target: NameTarget::New { then_add },
        };
    }

    /// The playlist a playlist-level command acts on: the open one, or the
    /// row selected on the Playlists tab.
    pub(crate) fn playlist_under_cursor(&self) -> Option<Playlist> {
        if self.queue_focused {
            return None;
        }
        match self.view_stack.last() {
            Some(View::PlaylistDetail(d)) => Some(d.playlist.clone()),
            Some(_) => None,
            None if self.current_tab == Tab::Playlists => self.playlists.selected_item().cloned(),
            None => None,
        }
    }

    pub(crate) fn own_playlist_under_cursor(&mut self) -> Option<Playlist> {
        let playlist = self.playlist_under_cursor()?;
        if !self.owned_playlist_ids.contains(&playlist.uuid) {
            self.set_status(
                "Only your own playlists can be edited".to_string(),
                StatusLevel::Info,
            );
            return None;
        }
        Some(playlist)
    }

    pub(crate) fn open_rename_prompt(&mut self) {
        let Some(playlist) = self.own_playlist_under_cursor() else {
            return;
        };
        self.name_prompt = NamePrompt {
            active: true,
            input: playlist.title.clone(),
            target: NameTarget::Rename(playlist),
        };
    }

    pub(crate) fn submit_name_prompt(&mut self) {
        let name = self.name_prompt.input.trim().to_string();
        if name.is_empty() {
            return;
        }
        let prompt = std::mem::take(&mut self.name_prompt);
        let request = match prompt.target {
            NameTarget::New { then_add } => ApiRequest::CreatePlaylist { name, then_add },
            NameTarget::Rename(playlist) if playlist.title == name => return,
            NameTarget::Rename(playlist) => ApiRequest::RenamePlaylist { playlist, name },
        };
        let _ = self.api_tx.send(request);
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
        let item_id = match outcome {
            AddOutcome::Added { item_id } => item_id,
            AddOutcome::Skipped { reason } => {
                self.set_status(
                    format!(
                        "Tidal didn't add '{}' to '{}' ({reason})",
                        track.title, playlist.title
                    ),
                    StatusLevel::Error,
                );
                return;
            }
        };
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
                    d.tracks.items.push(Track {
                        playlist_item_id: Some(item_id.clone()),
                        ..track.clone()
                    });
                    d.tracks.total = d.tracks.total.saturating_add(1);
                    d.tracks.refilter();
                    d.playlist.number_of_tracks =
                        Some(d.playlist.number_of_tracks.unwrap_or(0) + 1);
                }
            }
        }
        self.set_status(
            format!("Added '{}' to '{}'", track.title, playlist.title),
            StatusLevel::Info,
        );
    }

    pub(crate) fn remove_selected_from_playlist(&mut self) {
        let Some(View::PlaylistDetail(d)) = self.view_stack.last() else {
            return;
        };
        if d.focus != PlaylistDetailFocus::Tracks {
            return;
        }
        if !self.owned_playlist_ids.contains(&d.playlist.uuid) {
            self.set_status(
                "Only your own playlists can be edited".to_string(),
                StatusLevel::Info,
            );
            return;
        }
        let Some(track) = d.tracks.selected_item().cloned() else {
            return;
        };
        let Some(item_id) = track.playlist_item_id.clone() else {
            self.set_status(
                format!(
                    "Tidal sent no entry id for '{}', so it can't be removed",
                    track.title
                ),
                StatusLevel::Error,
            );
            return;
        };
        let _ = self.api_tx.send(ApiRequest::RemoveFromPlaylist {
            playlist: d.playlist.clone(),
            track,
            item_id,
        });
    }

    pub(crate) fn on_removed_from_playlist(
        &mut self,
        playlist: Playlist,
        track: Track,
        item_id: String,
    ) {
        if let Some(p) = self
            .playlists
            .items
            .iter_mut()
            .find(|p| p.uuid == playlist.uuid)
        {
            p.number_of_tracks = p.number_of_tracks.map(|n| n.saturating_sub(1));
        }
        for view in &mut self.view_stack {
            if let View::PlaylistDetail(d) = view
                && d.playlist.uuid == playlist.uuid
            {
                d.tracks
                    .remove_where(|t| t.playlist_item_id.as_deref() != Some(&item_id));
                // Not `tracks.total`: cursor pages arrive with a total of 0 and
                // `append` stores it, so on a multi-page playlist it reads 0.
                d.playlist.number_of_tracks =
                    d.playlist.number_of_tracks.map(|n| n.saturating_sub(1));
            }
        }
        self.set_status(
            format!("Removed '{}' from '{}'", track.title, playlist.title),
            StatusLevel::Info,
        );
    }

    pub(crate) fn on_playlist_renamed(&mut self, playlist: Playlist, name: String) {
        if let Some(p) = self
            .playlists
            .items
            .iter_mut()
            .find(|p| p.uuid == playlist.uuid)
        {
            p.title = name.clone();
            // The re-sort moves rows under a cursor that is only an index.
            let selected = self.playlists.selected_item().map(|p| p.uuid.clone());
            self.sort_playlists();
            if let Some(i) = selected.and_then(|uuid| {
                self.playlists
                    .visible_items()
                    .iter()
                    .position(|p| p.uuid == uuid)
            }) {
                self.playlists.selected = i;
            }
        }
        for view in &mut self.view_stack {
            if let View::PlaylistDetail(d) = view
                && d.playlist.uuid == playlist.uuid
            {
                d.playlist.title = name.clone();
            }
        }
        self.set_status(format!("Renamed to '{name}'"), StatusLevel::Info);
    }

    pub(crate) fn start_move(&mut self) {
        let Some(View::PlaylistDetail(d)) = self.view_stack.last() else {
            return;
        };
        if d.focus != PlaylistDetailFocus::Tracks {
            return;
        }
        // A drop beside a hidden row, or past rows still to arrive, would land
        // somewhere the user did not choose. Every entry needs its id because
        // the drop names its neighbour by it.
        let refusal = if !self.owned_playlist_ids.contains(&d.playlist.uuid) {
            Some(("Only your own playlists can be edited", StatusLevel::Info))
        } else if !d.tracks.exhausted {
            Some(("Wait for the playlist to finish loading", StatusLevel::Info))
        } else if d.tracks.visible_len() != d.tracks.items.len() {
            Some((
                "Show Atmos (Shift+S) to reorder: some tracks here are hidden",
                StatusLevel::Info,
            ))
        } else if d.tracks.items.iter().any(|t| t.playlist_item_id.is_none()) {
            Some((
                "Tidal sent no entry ids for this playlist, so it can't be reordered",
                StatusLevel::Error,
            ))
        } else {
            None
        };
        if let Some((msg, level)) = refusal {
            self.set_status(msg.to_string(), level);
            return;
        }
        let at = d.tracks.selected;
        let Some(item_id) = d
            .tracks
            .items
            .get(at)
            .and_then(|t| t.playlist_item_id.clone())
        else {
            return;
        };
        self.playlist_move = Some(PlaylistMove {
            item_id,
            origin_next: entry_after(&d.tracks, at),
        });
    }

    pub(crate) fn step_move(&mut self, down: bool) {
        let (Some(m), Some(View::PlaylistDetail(d))) =
            (&self.playlist_move, self.view_stack.last_mut())
        else {
            return;
        };
        let Some(from) = entry_position(&d.tracks, &m.item_id) else {
            return;
        };
        let to = if down {
            from + 1
        } else {
            let Some(to) = from.checked_sub(1) else {
                return;
            };
            to
        };
        if to >= d.tracks.items.len() {
            return;
        }
        d.tracks.items.swap(from, to);
        d.tracks.refilter();
        d.tracks.selected = to;
    }

    pub(crate) fn cancel_move(&mut self) {
        let Some(m) = self.playlist_move.take() else {
            return;
        };
        let Some(View::PlaylistDetail(d)) = self.view_stack.last_mut() else {
            return;
        };
        let Some(from) = entry_position(&d.tracks, &m.item_id) else {
            return;
        };
        let row = d.tracks.items.remove(from);
        let to = match &m.origin_next {
            None => Some(d.tracks.items.len()),
            Some(next) => entry_position(&d.tracks, next),
        };
        match to {
            Some(to) => {
                d.tracks.items.insert(to, row);
                d.tracks.refilter();
                d.tracks.selected = to;
            }
            // Its old neighbour was removed meanwhile, so there is no local
            // answer to where it was; Tidal still has the original order.
            None => {
                d.tracks.items.insert(from, row);
                let uuid = d.playlist.uuid.clone();
                self.reload_playlist(&uuid);
            }
        }
    }

    pub(crate) fn drop_move(&mut self) {
        let Some(m) = self.playlist_move.take() else {
            return;
        };
        let Some(View::PlaylistDetail(d)) = self.view_stack.last() else {
            return;
        };
        let Some(at) = entry_position(&d.tracks, &m.item_id) else {
            return;
        };
        let before = entry_after(&d.tracks, at);
        if before == m.origin_next {
            return;
        }
        let _ = self.api_tx.send(ApiRequest::MovePlaylistItem {
            playlist: d.playlist.clone(),
            track: d.tracks.items[at].clone(),
            item_id: m.item_id,
            before,
        });
    }

    /// Refetches the open view of `uuid`, the only sound recovery once the local
    /// order may differ from Tidal's.
    fn reload_playlist(&mut self, uuid: &str) {
        let Some(View::PlaylistDetail(d)) = self.view_stack.last_mut() else {
            return;
        };
        if d.playlist.uuid != uuid {
            return;
        }
        self.playlist_move = None;
        d.tracks = StatefulList::default();
        d.tracks.loading = true;
        self.apply_atmos_visibility();
        let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks {
            uuid: uuid.to_string(),
            next_url: None,
        });
    }

    pub(crate) fn on_playlist_item_moved(&mut self, _playlist: Playlist, track: Track) {
        self.set_status(format!("Moved '{}'", track.title), StatusLevel::Info);
    }

    pub(crate) fn on_playlist_item_move_failed(
        &mut self,
        playlist: Playlist,
        track: Track,
        error: String,
    ) {
        self.reload_playlist(&playlist.uuid);
        self.set_status(
            format!("Couldn't move '{}': {error}", track.title),
            StatusLevel::Error,
        );
    }

    pub(crate) fn open_delete_confirm(&mut self) {
        self.delete_confirm = self.own_playlist_under_cursor();
    }

    pub(crate) fn confirm_delete(&mut self) {
        if let Some(playlist) = self.delete_confirm.take() {
            let _ = self.api_tx.send(ApiRequest::DeletePlaylist { playlist });
        }
    }

    pub(crate) fn on_playlist_deleted(&mut self, playlist: Playlist) {
        self.playlists.remove_where(|p| p.uuid != playlist.uuid);
        self.favorite_playlist_ids.remove(&playlist.uuid);
        self.owned_playlist_ids.remove(&playlist.uuid);
        self.view_stack.retain(
            |view| !matches!(view, View::PlaylistDetail(d) if d.playlist.uuid == playlist.uuid),
        );
        self.set_status(format!("Deleted '{}'", playlist.title), StatusLevel::Info);
    }
}

#[cfg(test)]
mod tests {
    use crate::api::ApiRequest;
    use crate::api::models::{AddOutcome, Playlist};
    use crate::app::test_support::{TestApp, test_app, track};
    use crate::app::{NameTarget, StatusLevel, Tab, View};

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

    fn row(id: u64, item_id: &str) -> crate::api::models::Track {
        crate::api::models::Track {
            playlist_item_id: Some(item_id.to_string()),
            ..track(id)
        }
    }

    /// `own("p")` in the library and open, loaded with `rows`, owned by the user.
    fn own_playlist_open_with(rows: Vec<crate::api::models::Track>) -> TestApp {
        let mut t = test_app();
        t.app.playlists.append_page(vec![own("p", "Mine")], None);
        t.app.owned_playlist_ids.insert("p".to_string());
        t.app.open_playlist(own("p", "Mine"));
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks.append_page(rows, None);
        }
        t.drain_api();
        t
    }

    fn open_rows(t: &TestApp) -> Vec<Option<String>> {
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
            panic!("detail closed")
        };
        d.tracks
            .items
            .iter()
            .map(|t| t.playlist_item_id.clone())
            .collect()
    }

    /// Adding a row of a playlist back into that same playlist makes a second
    /// entry; keeping the source row's id would make `d` delete the original.
    #[test]
    fn an_added_row_carries_its_own_item_id_not_the_source_rows() {
        let mut t = own_playlist_open_with(vec![row(1, "a")]);
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            row(1, "a"),
            AddOutcome::Added {
                item_id: "b".to_string(),
            },
        );
        assert_eq!(open_rows(&t), [Some("a".into()), Some("b".into())]);
    }

    #[test]
    fn d_removes_the_entry_only_once_tidal_confirms() {
        let mut t = own_playlist_open_with(vec![row(1, "a"), row(2, "b")]);
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks.next();
        }
        t.app.remove_selected_from_playlist();
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::RemoveFromPlaylist { playlist, item_id, .. }]
                if playlist.uuid == "p" && item_id == "b"
        ));
        assert_eq!(open_rows(&t).len(), 2);

        t.app
            .on_removed_from_playlist(own("p", "Mine"), row(2, "b"), "b".to_string());
        assert_eq!(open_rows(&t), [Some("a".into())]);
        assert_eq!(t.app.playlists.items[0].number_of_tracks, Some(1));
        let (msg, _, _) = t.app.status.clone().unwrap();
        assert!(msg.contains("Removed"), "{msg}");
    }

    #[test]
    fn removing_one_copy_of_a_duplicate_keeps_the_other() {
        let mut t = own_playlist_open_with(vec![row(1, "a"), row(1, "b")]);
        t.app
            .on_removed_from_playlist(own("p", "Mine"), row(1, "a"), "a".to_string());
        assert_eq!(open_rows(&t), [Some("b".into())]);
    }

    #[test]
    fn d_on_a_narrowed_list_removes_the_highlighted_entry() {
        let mut t = own_playlist_open_with(vec![row(1, "a"), row(2, "b")]);
        if let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() {
            d.tracks.edit_filter(|q| q.push_str("Track 2"));
        }
        t.app.remove_selected_from_playlist();
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::RemoveFromPlaylist { item_id, .. }] if item_id == "b"
        ));
    }

    #[test]
    fn d_in_someone_elses_playlist_sends_nothing_and_says_why() {
        let mut t = own_playlist_open_with(vec![row(1, "a")]);
        t.app.owned_playlist_ids.clear();
        t.app.remove_selected_from_playlist();
        assert!(t.api_requests().is_empty());
        let (msg, _, _) = t.app.status.clone().unwrap();
        assert_eq!(msg, "Only your own playlists can be edited");
    }

    #[test]
    fn a_removal_confirmed_after_the_view_closed_still_fixes_the_count() {
        let mut t = test_app();
        t.app.playlists.append_page(vec![own("p", "Mine")], None);
        t.app
            .on_removed_from_playlist(own("p", "Mine"), row(1, "a"), "a".to_string());
        assert_eq!(t.app.playlists.items[0].number_of_tracks, Some(1));
    }

    /// Cursor pages arrive with a total of 0, which `append` stores in
    /// `tracks.total`; the header count must not be derived from it.
    #[test]
    fn edits_keep_the_header_count_of_a_playlist_loaded_over_two_pages() {
        let mut t = test_app();
        t.app.playlists.append_page(vec![own("p", "Mine")], None);
        t.app.owned_playlist_ids.insert("p".to_string());
        t.app.open_playlist(own("p", "Mine"));
        let page =
            |tracks, total, next_cursor: Option<&str>| crate::api::ApiResponse::PlaylistTracks {
                uuid: "p".to_string(),
                tracks,
                total,
                next_cursor: next_cursor.map(str::to_string),
                description: None,
                cover: None,
            };
        t.app
            .handle_api_response(page(vec![row(1, "a"), row(2, "b")], 3, Some("c")));
        t.app.handle_api_response(page(vec![row(3, "c")], 0, None));
        let header = |t: &TestApp| {
            let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
                panic!("detail closed")
            };
            d.playlist.number_of_tracks
        };
        assert_eq!(header(&t), Some(3));

        t.app
            .on_removed_from_playlist(own("p", "Mine"), row(3, "c"), "c".to_string());
        assert_eq!(header(&t), Some(2));
        t.app.on_added_to_playlist(
            own("p", "Mine"),
            track(4),
            AddOutcome::Added {
                item_id: "d".to_string(),
            },
        );
        assert_eq!(header(&t), Some(3));
    }

    fn on_playlists_tab_with(p: Playlist, owned: bool) -> TestApp {
        let mut t = test_app();
        t.app.current_tab = Tab::Playlists;
        if owned {
            t.app.owned_playlist_ids.insert(p.uuid.clone());
        }
        t.app.playlists.append_page(vec![p], None);
        t.drain_api();
        t
    }

    #[test]
    fn r_opens_the_prompt_prefilled_with_the_current_name() {
        let mut t = on_playlists_tab_with(own("p", "Mine"), true);
        t.app.open_rename_prompt();
        assert!(t.app.name_prompt.active);
        assert_eq!(t.app.name_prompt.input, "Mine");
        assert!(matches!(&t.app.name_prompt.target, NameTarget::Rename(p) if p.uuid == "p"));
    }

    #[test]
    fn renaming_sends_the_new_name_and_applies_it_on_confirm() {
        let mut t = on_playlists_tab_with(own("p", "Mine"), true);
        t.app.open_playlist(own("p", "Mine"));
        t.drain_api();
        t.app.open_rename_prompt();
        t.app.name_prompt.input = "  Road trip ".to_string();
        t.app.submit_name_prompt();
        assert!(!t.app.name_prompt.active);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::RenamePlaylist { playlist, name }]
                if playlist.uuid == "p" && name == "Road trip"
        ));
        assert_eq!(t.app.playlists.items[0].title, "Mine");

        t.app
            .on_playlist_renamed(own("p", "Mine"), "Road trip".to_string());
        assert_eq!(t.app.playlists.items[0].title, "Road trip");
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last() else {
            panic!("detail closed")
        };
        assert_eq!(d.playlist.title, "Road trip");
    }

    #[test]
    fn renaming_to_the_same_or_a_blank_name_sends_nothing() {
        let mut t = on_playlists_tab_with(own("p", "Mine"), true);
        t.app.open_rename_prompt();
        t.app.name_prompt.input = " Mine ".to_string();
        t.app.submit_name_prompt();
        assert!(!t.app.name_prompt.active);

        t.app.open_rename_prompt();
        t.app.name_prompt.input = "   ".to_string();
        t.app.submit_name_prompt();
        assert!(t.app.name_prompt.active);
        assert!(t.api_requests().is_empty());
    }

    #[test]
    fn r_on_someone_elses_playlist_opens_nothing_and_says_why() {
        let mut t = on_playlists_tab_with(own("p", "Theirs"), false);
        t.app.open_rename_prompt();
        assert!(!t.app.name_prompt.active);
        let (msg, _, _) = t.app.status.clone().unwrap();
        assert_eq!(msg, "Only your own playlists can be edited");
    }

    #[test]
    fn with_the_queue_focused_no_playlist_is_under_the_cursor() {
        let mut t = on_playlists_tab_with(own("p", "Mine"), true);
        t.app.queue_focused = true;
        assert!(t.app.playlist_under_cursor().is_none());
    }

    #[test]
    fn d_asks_first_and_deletes_only_on_y() {
        let mut t = on_playlists_tab_with(own("p", "Mine"), true);
        t.app.favorite_playlist_ids.insert("p".to_string());
        t.app.open_delete_confirm();
        assert!(matches!(&t.app.delete_confirm, Some(p) if p.uuid == "p"));
        assert!(t.api_requests().is_empty());

        t.app.confirm_delete();
        assert!(t.app.delete_confirm.is_none());
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::DeletePlaylist { playlist }] if playlist.uuid == "p"
        ));
        assert_eq!(t.app.playlists.items.len(), 1);

        t.app.on_playlist_deleted(own("p", "Mine"));
        assert!(t.app.playlists.items.is_empty());
        assert!(!t.app.favorite_playlist_ids.contains("p"));
        assert!(!t.app.owned_playlist_ids.contains("p"));
        let (msg, _, _) = t.app.status.clone().unwrap();
        assert!(msg.contains("Deleted"), "{msg}");
    }

    #[test]
    fn deleting_a_playlist_closes_every_view_of_it_and_nothing_else() {
        let mut t = on_playlists_tab_with(own("p", "Mine"), true);
        t.app.open_playlist(own("q", "Other"));
        t.app.open_playlist(own("p", "Mine"));
        t.app.open_artist(crate::api::models::Artist {
            id: 1,
            name: "Someone".to_string(),
            added_at: None,
        });
        t.app.on_playlist_deleted(own("p", "Mine"));
        assert!(matches!(
            t.app.view_stack.as_slice(),
            [View::PlaylistDetail(q), View::ArtistDetail(_)] if q.playlist.uuid == "q"
        ));
    }

    #[test]
    fn d_on_someone_elses_playlist_asks_nothing() {
        let mut t = on_playlists_tab_with(own("p", "Theirs"), false);
        t.app.open_delete_confirm();
        assert!(t.app.delete_confirm.is_none());
        let (msg, _, _) = t.app.status.clone().unwrap();
        assert_eq!(msg, "Only your own playlists can be edited");
    }

    #[test]
    fn the_cursor_follows_a_renamed_playlist_to_its_new_place() {
        let mut t = on_playlists_tab_with(own("z", "Zebra"), true);
        t.app.playlists.items.insert(0, own("m", "Middle"));
        t.app.playlists.refilter();
        t.app.playlists.selected = 1;
        t.app
            .on_playlist_renamed(own("z", "Zebra"), "Apple".to_string());
        assert_eq!(
            t.app.playlists.selected_item().map(|p| p.uuid.as_str()),
            Some("z")
        );
    }

    fn abc() -> TestApp {
        own_playlist_open_with(vec![row(1, "a"), row(2, "b"), row(3, "c")])
    }

    fn ids(t: &TestApp) -> Vec<String> {
        open_rows(t).into_iter().map(Option::unwrap).collect()
    }

    fn detail_mut(t: &mut TestApp) -> &mut crate::app::PlaylistDetail {
        let Some(View::PlaylistDetail(d)) = t.app.view_stack.last_mut() else {
            panic!("detail closed")
        };
        d
    }

    #[test]
    fn dropping_sends_one_move_naming_the_next_row() {
        let mut t = abc();
        t.app.start_move();
        t.app.step_move(true);
        assert_eq!(ids(&t), ["b", "a", "c"]);
        assert_eq!(detail_mut(&mut t).tracks.selected, 1);
        assert!(t.api_requests().is_empty());
        t.app.drop_move();
        assert!(t.app.playlist_move.is_none());
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::MovePlaylistItem { item_id, before: Some(before), .. }]
                if item_id == "a" && before == "c"
        ));

        t.app.start_move();
        t.app.step_move(true);
        assert_eq!(ids(&t), ["b", "c", "a"]);
        t.app.drop_move();
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::MovePlaylistItem { item_id, before: None, .. }]
                if item_id == "a"
        ));
    }

    #[test]
    fn dropping_where_it_started_sends_nothing() {
        let mut t = abc();
        t.app.start_move();
        t.app.step_move(true);
        t.app.step_move(false);
        t.app.drop_move();
        assert!(t.app.playlist_move.is_none());
        assert!(t.api_requests().is_empty());
    }

    /// Replaying the old index is wrong once a second move or an add or remove
    /// has landed in between; only Tidal knows the order now.
    #[test]
    fn a_failed_move_reloads_the_playlist_from_tidal() {
        let mut t = abc();
        t.app.start_move();
        t.app.step_move(true);
        t.app.drop_move();
        t.drain_api();
        t.app.start_move();
        t.app
            .on_playlist_item_move_failed(own("p", "Mine"), track(1), "Nope".to_string());
        assert!(t.app.playlist_move.is_none());
        assert!(open_rows(&t).is_empty());
        assert!(detail_mut(&mut t).tracks.loading);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::LoadPlaylistTracks { uuid, next_url: None }] if uuid == "p"
        ));
        let (msg, level, _) = t.app.status.clone().unwrap();
        assert_eq!(level, StatusLevel::Error);
        assert!(msg.contains("Couldn't move"), "{msg}");
    }

    #[test]
    fn move_mode_is_refused_only_when_rows_are_really_hidden() {
        let mut t = abc();
        detail_mut(&mut t).tracks.set_hide_atmos(true);
        t.app.start_move();
        assert!(t.app.playlist_move.is_some());
        t.app.cancel_move();

        let tracks = &mut detail_mut(&mut t).tracks;
        tracks.items.push(crate::api::models::Track {
            playlist_item_id: Some("x".to_string()),
            ..crate::app::test_support::track_tagged(9, &["DOLBY_ATMOS"])
        });
        tracks.refilter();
        t.app.start_move();
        assert!(t.app.playlist_move.is_none());
        let (msg, _, _) = t.app.status.clone().unwrap();
        assert_eq!(
            msg,
            "Show Atmos (Shift+S) to reorder: some tracks here are hidden"
        );
    }

    #[test]
    fn moving_stops_at_both_ends() {
        let mut t = abc();
        t.app.start_move();
        t.app.step_move(false);
        assert_eq!(ids(&t), ["a", "b", "c"]);
        t.app.cancel_move();
        detail_mut(&mut t).tracks.selected = 2;
        t.app.start_move();
        t.app.step_move(true);
        assert_eq!(ids(&t), ["a", "b", "c"]);
    }

    #[test]
    fn m_on_someone_elses_playlist_or_a_loading_one_refuses() {
        let mut t = abc();
        t.app.owned_playlist_ids.clear();
        t.app.start_move();
        assert!(t.app.playlist_move.is_none());
        assert_eq!(
            t.app.status.clone().unwrap().0,
            "Only your own playlists can be edited"
        );

        let mut t = abc();
        detail_mut(&mut t).tracks.exhausted = false;
        t.app.start_move();
        assert!(t.app.playlist_move.is_none());
        assert_eq!(
            t.app.status.clone().unwrap().0,
            "Wait for the playlist to finish loading"
        );
    }

    #[test]
    fn a_view_pushed_mid_move_puts_the_row_back_first() {
        let mut t = abc();
        t.app.start_move();
        t.app.step_move(true);
        t.app.open_artist(crate::api::models::Artist {
            id: 1,
            name: "Someone".to_string(),
            added_at: None,
        });
        assert!(t.app.playlist_move.is_none());
        t.app.go_back();
        assert_eq!(ids(&t), ["a", "b", "c"]);
    }

    #[test]
    fn a_removal_landing_mid_move_leaves_cancel_and_drop_honest() {
        let mut t =
            own_playlist_open_with(vec![row(1, "a"), row(2, "b"), row(3, "c"), row(4, "d")]);
        detail_mut(&mut t).tracks.selected = 2;
        t.app.start_move();
        t.app
            .on_removed_from_playlist(own("p", "Mine"), row(1, "a"), "a".to_string());
        t.app.drop_move();
        assert!(t.api_requests().is_empty());

        detail_mut(&mut t).tracks.selected = 1;
        t.app.start_move();
        t.app
            .on_removed_from_playlist(own("p", "Mine"), row(2, "b"), "b".to_string());
        t.app.cancel_move();
        assert_eq!(ids(&t), ["c", "d"]);
    }
}
