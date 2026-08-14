// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist};
use super::{App, ArtistDetail, ArtistDetailFocus, AlbumDetail, PlaylistDetail, PlaylistDetailFocus, StatefulList, Tab, View};

impl App {
    // ── Tab switching ─────────────────────────────────────────────────────────

    pub fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Home      => Tab::Favorites,
            Tab::Favorites => Tab::Artists,
            Tab::Artists   => Tab::Albums,
            Tab::Albums    => Tab::Playlists,
            Tab::Playlists => Tab::Search,
            Tab::Search    => Tab::Home,
        };
        self.art_fullscreen = false;
        self.view_stack.clear();
        self.on_tab_entered();
    }

    pub fn prev_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Home      => Tab::Search,
            Tab::Favorites => Tab::Home,
            Tab::Artists   => Tab::Favorites,
            Tab::Albums    => Tab::Artists,
            Tab::Playlists => Tab::Albums,
            Tab::Search    => Tab::Playlists,
        };
        self.art_fullscreen = false;
        self.view_stack.clear();
        self.on_tab_entered();
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
        self.art_fullscreen = false;
        self.view_stack.clear();
        self.on_tab_entered();
    }

    /// Show album art without disturbing the tab or detail view underneath it.
    pub fn enter_art_fullscreen(&mut self) {
        if self.art_fullscreen {
            return;
        }
        self.art_fullscreen = true;
        self.queue_focused = false;
        self.fetch_presentation_art();
    }

    pub fn exit_art_fullscreen(&mut self) {
        self.art_fullscreen = false;
    }

    pub fn toggle_art_fullscreen(&mut self) {
        if self.art_fullscreen {
            self.exit_art_fullscreen();
        } else {
            self.enter_art_fullscreen();
        }
    }

    /// Landing on Search drops straight into the query box, but only when there
    /// are no results to come back to. Otherwise the previous results stay
    /// browsable and `/` reopens the box. The query is deliberately left intact
    /// so tabbing away and back doesn't discard what was typed; `/` is the
    /// explicit "start a new search" action and clears it.
    fn on_tab_entered(&mut self) {
        // Switching tabs means the user wants the new tab's content, so release
        // the queue — otherwise it keeps capturing the arrow keys from behind.
        self.queue_focused = false;

        if self.current_tab == Tab::Search && !self.search.has_results() {
            self.search.modal_open = true;
        }
    }

    // ── View stack ────────────────────────────────────────────────────────────

    pub fn go_back(&mut self) {
        if !self.view_stack.is_empty() {
            self.view_stack.pop();
        }
    }

    // ── Opening views ─────────────────────────────────────────────────────────

    pub fn open_selected_artist(&mut self) {
        let Some(artist) = self.artists.selected_item().cloned() else { return };
        self.open_artist(artist);
    }

    pub fn open_artist(&mut self, artist: Artist) {
        tracing::debug!("Fetching details for artist {} [{}]", artist.name, artist.id);
        let id = artist.id;
        let mut tracks = StatefulList::default();
        tracks.loading = true;
        let detail = ArtistDetail {
            artist,
            tracks,
            albums: StatefulList::default(),
            eps: StatefulList::default(),
            singles: StatefulList::default(),
            focus: ArtistDetailFocus::Tracks,
            art_bytes: None,
            art_loading: false,
            bio: None,
            bio_loading: true,
            bio_scroll: 0,
        };
        self.view_stack.push(View::ArtistDetail(detail));
        let _ = self.api_tx.send(ApiRequest::LoadArtistTopTracks { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistBio      { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistPicture  { artist_id: id });
    }

    pub fn open_album(&mut self, album: Album) {
        let album_id = album.id;
        self.view_stack.push(View::AlbumDetail(AlbumDetail {
            album,
            tracks: StatefulList::default(),
            art_bytes: None,
            art_loading: true,
        }));
        let _ = self.api_tx.send(ApiRequest::LoadAlbum       { album_id });
        let _ = self.api_tx.send(ApiRequest::LoadAlbumTracks { album_id });
        // Don't fetch cover here - let AlbumLoaded handler fetch with the correct cover ID
    }

    pub fn open_selected_album(&mut self) {
        let album = if let Some(View::ArtistDetail(detail)) = self.view_stack.last() {
            match detail.focus {
                ArtistDetailFocus::Albums => detail.albums.selected_item().cloned(),
                ArtistDetailFocus::EPs => detail.eps.selected_item().cloned(),
                ArtistDetailFocus::Singles => detail.singles.selected_item().cloned(),
                _ => None,
            }
        } else {
            None
        };
        if let Some(album) = album { self.open_album(album); }
    }

    pub fn open_selected_fav_album(&mut self) {
        if let Some(album) = self.fav_albums.selected_item().cloned() {
            self.open_album(album);
        }
    }

    pub fn open_playlist(&mut self, playlist: Playlist) {
        let uuid = playlist.uuid.clone();
        let mut tracks: StatefulList<crate::api::models::Track> = StatefulList::default();
        tracks.loading = true;
        // Show spinner for mixes (from Home tab) even if cover isn't loaded yet
        let is_home_mix = self.current_tab == Tab::Home;
        let has_cover = playlist.cover.is_some();
        let art_loading = has_cover || is_home_mix;
        let detail = PlaylistDetail {
            playlist: playlist.clone(),
            tracks,
            focus: PlaylistDetailFocus::Tracks,
            art_bytes: None,
            art_loading,
            description_scroll: 0,
        };
        self.view_stack.push(View::PlaylistDetail(detail));
        // Use v2 API for mixes from Home tab, v1 for regular playlists
        if self.current_tab == Tab::Home {
            let _ = self.api_tx.send(ApiRequest::LoadMixTracks { uuid, offset: 0 });
        } else {
            let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks { uuid, next_url: None });
        }
        if let Some(cover_url) = playlist.cover {
            let uuid_for_art = playlist.uuid.clone();
            let _ = self.api_tx.send(ApiRequest::FetchPlaylistArt { uuid: uuid_for_art, cover_url });
        }
    }

    pub fn open_selected_playlist(&mut self) {
        let Some(playlist) = self.playlists.selected_item().cloned() else { return };
        self.open_playlist(playlist);
    }

    pub fn open_selected_home_item(&mut self) {
        use super::HomeSectionFocus;
        match self.home_section_focus {
            HomeSectionFocus::NewReleases => {
                if let Some(playlist) = self.home_new_releases.selected_item().cloned() {
                    self.open_playlist(playlist);
                }
            }
            HomeSectionFocus::DailyMixes => {
                if let Some(playlist) = self.home_daily_mixes.selected_item().cloned() {
                    self.open_playlist(playlist);
                }
            }
            HomeSectionFocus::DiscoveryMixes => {
                if let Some(playlist) = self.home_discovery_mixes.selected_item().cloned() {
                    self.open_playlist(playlist);
                }
            }
        }
    }

    pub fn go_to_artist_from_track(&mut self, track: &crate::api::models::Track) {
        let artist_names: Vec<String> = if !track.artists.is_empty() {
            track.artists.iter().map(|a| a.name.clone()).collect()
        } else if let Some(ref artist) = track.artist {
            vec![artist.name.clone()]
        } else {
            Vec::new()
        };

        if artist_names.is_empty() {
            self.set_status("No artist information available".to_string(), crate::app::StatusLevel::Error);
            return;
        }

        if artist_names.len() == 1 {
            let name = artist_names[0].clone();
            self.artist_selection.searching_for = Some(name.clone());
            let _ = self.api_tx.send(ApiRequest::SearchArtistByName {
                query: name,
            });
        } else {
            self.artist_selection.artist_names = artist_names;
            self.artist_selection.selected = 0;
            self.artist_selection.active = true;
        }
    }

    pub fn open_selected_artist_from_selection(&mut self) {
        if let Some(name) = self.artist_selection.artist_names.get(self.artist_selection.selected).cloned() {
            self.artist_selection.active = false;
            self.artist_selection.searching_for = Some(name.clone());
            self.artist_selection.artist_names.clear();
            let _ = self.api_tx.send(ApiRequest::SearchArtistByName {
                query: name,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiRequest;
    use crate::api::models::{Album, ArtistRef, Track};
    use crate::app::Preferences;
    use crate::lastfm::LastfmCmd;
    use crate::mpris::MprisState;
    use crate::player::PlayerCmd;
    use tokio::sync::{mpsc, watch};

    fn make_app() -> (App, mpsc::UnboundedReceiver<ApiRequest>) {
        let (api_tx, api_rx) = mpsc::unbounded_channel();
        let (player_tx, _): (mpsc::UnboundedSender<PlayerCmd>, _) = mpsc::unbounded_channel();
        let (mpris_tx, _) = watch::channel(MprisState::default());
        let (lastfm_tx, _): (mpsc::UnboundedSender<LastfmCmd>, _) = mpsc::unbounded_channel();
        (
            App::new(api_tx, player_tx, mpris_tx, lastfm_tx, Preferences::default()),
            api_rx,
        )
    }

    fn track() -> Track {
        Track {
            id: 1,
            title: "Track".to_string(),
            duration: 180,
            artist: Some(ArtistRef { name: "Artist".to_string() }),
            artists: Vec::new(),
            album: Album {
                id: 2,
                title: "Album".to_string(),
                number_of_tracks: None,
                release_date: None,
                cover: None,
                artist: None,
                audio_quality: None,
                media_metadata: None,
                added_at: None,
                album_type: None,
            },
            audio_quality: None,
            media_metadata: None,
            added_at: None,
        }
    }

    #[test]
    fn tabs_wrap_between_search_and_home_in_both_directions() {
        let (mut app, _) = make_app();
        app.current_tab = Tab::Search;
        app.queue_focused = true;

        app.next_tab();
        assert_eq!(app.current_tab, Tab::Home);
        assert!(!app.queue_focused);

        app.prev_tab();
        assert_eq!(app.current_tab, Tab::Search);
    }

    #[test]
    fn fullscreen_art_preserves_the_current_view_and_requests_art_on_demand() {
        let (mut app, mut api_rx) = make_app();
        while api_rx.try_recv().is_ok() {}
        app.now_playing.track = Some(track());
        app.now_playing.art_source = Some("cover-id".to_string());
        app.current_tab = Tab::Albums;
        app.open_album(track().album);
        while api_rx.try_recv().is_ok() {}

        app.enter_art_fullscreen();

        assert!(app.art_fullscreen);
        assert_eq!(app.current_tab, Tab::Albums);
        assert_eq!(app.view_stack.len(), 1);
        assert!(app.now_playing.presentation_art_loading);
        assert!(matches!(
            api_rx.try_recv(),
            Ok(ApiRequest::FetchPresentationArt { album_id: 2, cover_id })
                if cover_id == "cover-id"
        ));

        app.toggle_art_fullscreen();
        assert!(!app.art_fullscreen);
        assert_eq!(app.current_tab, Tab::Albums);
        assert_eq!(app.view_stack.len(), 1);
    }
}
