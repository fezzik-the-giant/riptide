// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::ApiRequest;
use super::{App, View};

impl App {
    pub fn load_artists(&mut self) {
        if self.artists.loading || self.artists.exhausted { return; }
        self.artists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadArtists);
    }

    pub fn load_fav_albums(&mut self) {
        if self.fav_albums.loading || self.fav_albums.exhausted { return; }
        self.fav_albums.loading = true;
        let next_url = self.fav_albums.pagination_cursor.clone();
        let _ = self.api_tx.send(ApiRequest::LoadFavAlbums { next_url });
    }

    pub fn load_playlists(&mut self) {
        if self.playlists.loading || self.playlists.exhausted { return; }
        self.playlists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadPlaylists { offset: self.playlists.next_offset });
    }

    pub fn load_favorites(&mut self) {
        if self.favorites.loading || self.favorites.exhausted { return; }
        self.favorites.loading = true;
        self.favorites.last_load_triggered_at = self.favorites.items.len();
        let _ = self.api_tx.send(ApiRequest::LoadFavorites);
    }

    pub fn load_more_playlist_tracks(&mut self) {
        if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
            tracing::debug!("load_more_playlist_tracks check: loading={}, exhausted={}", detail.tracks.loading, detail.tracks.exhausted);
            if !detail.tracks.loading && !detail.tracks.exhausted {
                let uuid = detail.playlist.uuid.clone();
                let next_url = detail.tracks.pagination_cursor.clone();
                detail.tracks.loading = true;
                match &next_url {
                    Some(_) => tracing::debug!("Sending request for next page"),
                    None => tracing::debug!("Sending request for initial page"),
                }
                let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks { uuid, next_url });
            } else {
                if detail.tracks.loading {
                    tracing::debug!("Skipping load - already loading");
                }
                if detail.tracks.exhausted {
                    tracing::debug!("Skipping load - exhausted");
                }
            }
        } else {
            tracing::debug!("load_more_playlist_tracks: no detail view open");
        }
    }

    pub fn load_more_artist_tracks(&mut self) {
        if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
            if !detail.tracks.loading && !detail.tracks.exhausted {
                let artist_id = detail.artist.id;
                detail.tracks.loading = true;
                let _ = self.api_tx.send(ApiRequest::LoadArtistTopTracks { artist_id });
            }
        }
    }

    pub fn load_more_artist_albums(&mut self) {
        if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
            if !detail.albums.loading && !detail.albums.exhausted {
                let artist_id = detail.artist.id;
                detail.albums.loading = true;
                let _ = self.api_tx.send(ApiRequest::LoadArtistAlbums { artist_id });
            }
        }
    }

    pub fn load_more_artist_eps(&mut self) {
        if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
            if !detail.eps.loading && !detail.eps.exhausted {
                let artist_id = detail.artist.id;
                detail.eps.loading = true;
                let _ = self.api_tx.send(ApiRequest::LoadArtistEPs { artist_id });
            }
        }
    }

    pub fn load_more_artist_singles(&mut self) {
        if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
            if !detail.singles.loading && !detail.singles.exhausted {
                let artist_id = detail.artist.id;
                detail.singles.loading = true;
                let _ = self.api_tx.send(ApiRequest::LoadArtistSingles { artist_id });
            }
        }
    }

    pub(crate) fn fetch_now_playing_metadata(&mut self) {
        self.fetch_now_playing_art();
        self.fetch_lyrics();
    }

    fn fetch_now_playing_art(&mut self) {
        let (album_id, cover_id) = match &self.now_playing.track {
            Some(t) => (t.album.id, t.album.cover.clone()),
            None => return,
        };
        self.now_playing.art_bytes = None;
        tracing::debug!("fetch_now_playing_art: album_id={}, cover={:?}", album_id, cover_id);
        if let Some(cover_id) = cover_id {
            self.now_playing.art_loading = true;
            let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id });
        } else if album_id > 0 {
            // Album cover not available; fetch album to get cover art
            tracing::debug!("No cover available, fetching album to get cover");
            self.now_playing.art_loading = true;
            let _ = self.api_tx.send(ApiRequest::LoadAlbum { album_id });
        } else {
            self.now_playing.art_loading = false;
        }
    }

    fn fetch_lyrics(&mut self) {
        let Some(track) = &self.now_playing.track else { return };
        let track_id = track.id;
        self.now_playing.lyrics_synced = Vec::new();
        self.now_playing.lyrics_plain = Vec::new();
        self.now_playing.lyrics_loading = true;
        let _ = self.api_tx.send(ApiRequest::FetchLyrics { track_id });
    }
}
