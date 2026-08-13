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
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.modal_open = true;
            self.search.query.clear();
        }
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
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.modal_open = true;
            self.search.query.clear();
        }
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.modal_open = true;
            self.search.query.clear();
        }
    }

    // ── View stack ────────────────────────────────────────────────────────────

    pub fn go_back(&mut self) {
        if self.search.active {
            self.search.active = false;
            return;
        }
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
