// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use super::{App, View};
use crate::api::ApiRequest;

impl App {
    pub fn load_artists(&mut self) {
        if self.artists.loading || self.artists.exhausted {
            return;
        }
        self.artists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadArtists);
    }

    pub fn load_fav_albums(&mut self) {
        if self.fav_albums.loading || self.fav_albums.exhausted {
            return;
        }
        self.fav_albums.loading = true;
        let next_url = self.fav_albums.pagination_cursor.clone();
        let _ = self.api_tx.send(ApiRequest::LoadFavAlbums { next_url });
    }

    pub fn load_playlists(&mut self) {
        if self.playlists.loading || self.playlists.exhausted {
            return;
        }
        self.playlists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadPlaylists);
    }

    pub fn load_favorites(&mut self) {
        if self.favorites.loading || self.favorites.exhausted {
            return;
        }
        self.favorites.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadFavorites);
    }

    pub fn load_more_playlist_tracks(&mut self) {
        if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
            if !detail.tracks.loading && !detail.tracks.exhausted {
                let uuid = detail.playlist.uuid.clone();
                let next_url = detail.tracks.pagination_cursor.clone();
                detail.tracks.loading = true;
                let _ = self
                    .api_tx
                    .send(ApiRequest::LoadPlaylistTracks { uuid, next_url });
            }
        }
    }

    pub(crate) fn fetch_now_playing_metadata(&mut self) {
        self.fetch_now_playing_art();
        if self.art_fullscreen {
            self.fetch_presentation_art();
        }
        self.fetch_lyrics();
    }

    fn fetch_now_playing_art(&mut self) {
        let (album_id, cover_id) = match &self.now_playing.track {
            Some(t) => (t.album.id, t.album.cover.clone()),
            None => return,
        };
        self.now_playing.set_art_bytes(None);
        self.now_playing.art_source = cover_id.clone();
        self.now_playing.set_presentation_art_bytes(None);
        self.now_playing.finish_presentation_art_load();
        if let Some(cover_id) = cover_id {
            self.now_playing.art_loading = true;
            let _ = self
                .api_tx
                .send(ApiRequest::FetchAlbumArt { album_id, cover_id });
        } else if album_id > 0 {
            // Album cover not available; fetch album to get cover art
            self.now_playing.art_loading = true;
            let _ = self.api_tx.send(ApiRequest::LoadAlbum { album_id });
        } else {
            self.now_playing.art_loading = false;
        }
    }

    pub(crate) fn fetch_presentation_art(&mut self) {
        if self.now_playing.presentation_art_bytes().is_some()
            || self.now_playing.presentation_art_loading()
        {
            return;
        }

        let Some(track) = &self.now_playing.track else {
            return;
        };
        let album_id = track.album.id;
        let cover = self
            .now_playing
            .art_source
            .clone()
            .or_else(|| track.album.cover.clone());

        if let Some(cover_id) = cover {
            self.now_playing.art_source = Some(cover_id.clone());
            self.now_playing.begin_presentation_art_fetch();
            let _ = self
                .api_tx
                .send(ApiRequest::FetchPresentationArt { album_id, cover_id });
        } else if album_id > 0 && !self.now_playing.art_loading {
            self.now_playing.begin_presentation_art_discovery();
            let _ = self.api_tx.send(ApiRequest::LoadAlbum { album_id });
        }
    }

    fn fetch_lyrics(&mut self) {
        let Some(track) = &self.now_playing.track else {
            return;
        };
        let track_id = track.id;
        self.now_playing.lyrics_synced = Vec::new();
        self.now_playing.lyrics_plain = Vec::new();
        self.now_playing.lyrics_loading = true;
        let _ = self.api_tx.send(ApiRequest::FetchLyrics { track_id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Album, ArtistRef, Track};
    use crate::app::test_app;

    fn track(cover: Option<&str>) -> Track {
        Track {
            id: 1,
            title: "Track".to_string(),
            duration: 180,
            artist: Some(ArtistRef {
                name: "Artist".to_string(),
            }),
            artists: Vec::new(),
            album: Album {
                id: 2,
                title: "Album".to_string(),
                number_of_tracks: None,
                release_date: None,
                cover: cover.map(str::to_string),
                artist: None,
                media_metadata: None,
                added_at: None,
                album_type: None,
            },
            media_metadata: None,
            added_at: None,
        }
    }

    #[test]
    fn presentation_art_fetch_is_idempotent_while_loading() {
        let (mut app, mut api_rx) = test_app();
        while api_rx.try_recv().is_ok() {}
        app.now_playing.track = Some(track(Some("cover-id")));

        app.fetch_presentation_art();
        app.fetch_presentation_art();

        assert!(app.now_playing.presentation_art_loading());
        assert!(matches!(
            api_rx.try_recv(),
            Ok(ApiRequest::FetchPresentationArt { album_id: 2, cover_id })
                if cover_id == "cover-id"
        ));
        assert!(api_rx.try_recv().is_err());
    }

    #[test]
    fn missing_cover_loads_album_once_and_keeps_loading_visible() {
        let (mut app, mut api_rx) = test_app();
        while api_rx.try_recv().is_ok() {}
        app.now_playing.track = Some(track(None));

        app.fetch_presentation_art();
        app.fetch_presentation_art();

        assert!(app.now_playing.presentation_art_loading());
        assert!(matches!(
            api_rx.try_recv(),
            Ok(ApiRequest::LoadAlbum { album_id: 2 })
        ));
        assert!(api_rx.try_recv().is_err());
    }
}
