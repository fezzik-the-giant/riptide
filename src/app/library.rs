// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use super::{App, SortField, SortPalette, StatusLevel, Tab};
use crate::api::ApiRequest;
use crate::api::models::{Album, Artist, Playlist, Track};

impl App {
    // ── Home ──────────────────────────────────────────────────────────────────

    pub fn load_home(&mut self) {
        self.home_new_releases.loading = true;
        self.home_daily_mixes.loading = true;
        self.home_discovery_mixes.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadNewReleases);
        let _ = self.api_tx.send(ApiRequest::LoadDailyMixes);
        let _ = self.api_tx.send(ApiRequest::LoadDiscoveryMixes);
    }

    // ── Favorites ─────────────────────────────────────────────────────────────

    fn favorite_track(&mut self, track: &Track) {
        let _ = self
            .api_tx
            .send(ApiRequest::FavoriteTrack { track_id: track.id });
        if !self.favorites.items.iter().any(|t| t.id == track.id) {
            self.favorites.items.insert(0, track.clone());
            self.favorites.total = self.favorites.total.saturating_add(1);
            self.favorites.selected = self.favorites.selected.saturating_add(1);
            self.favorites.refilter();
            self.rebuild_favorite_track_ids();
        }
        self.set_status(
            format!("Added '{}' to favorites", track.title),
            StatusLevel::Info,
        );
    }

    fn unfavorite_track(&mut self, track: &Track) {
        let _ = self
            .api_tx
            .send(ApiRequest::UnfavoriteTrack { track_id: track.id });
        self.set_status(
            format!("Removed '{}' from favorites", track.title),
            StatusLevel::Info,
        );
    }

    pub fn toggle_favorite_track(&mut self, track: &Track) {
        if self.favorites.items.iter().any(|t| t.id == track.id) {
            self.unfavorite_track(track);
        } else {
            self.favorite_track(track);
        }
    }

    // ── Following ─────────────────────────────────────────────────────────────

    fn follow_artist(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::FollowArtist {
            artist_id: artist.id,
        });
        if !self.artists.items.iter().any(|a| a.id == artist.id) {
            let pos = self
                .artists
                .items
                .partition_point(|a| a.name.to_lowercase() < artist.name.to_lowercase());
            self.artists.items.insert(pos, artist.clone());
            self.artists.total = self.artists.total.saturating_add(1);
            if pos <= self.artists.selected {
                self.artists.selected = self.artists.selected.saturating_add(1);
            }
            self.artists.refilter();
        }
        self.set_status(format!("Following {}", artist.name), StatusLevel::Info);
    }

    fn unfollow_artist(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::UnfollowArtist {
            artist_id: artist.id,
        });
        self.set_status(format!("Unfollowed {}", artist.name), StatusLevel::Info);
    }

    pub fn toggle_follow_artist(&mut self, artist: &Artist) {
        if self.artists.items.iter().any(|a| a.id == artist.id) {
            self.unfollow_artist(artist);
        } else {
            self.follow_artist(artist);
        }
    }

    // ── Albums ────────────────────────────────────────────────────────────────

    fn favorite_album(&mut self, album: &Album) {
        let _ = self
            .api_tx
            .send(ApiRequest::FavoriteAlbum { album_id: album.id });
        if !self.fav_albums.items.iter().any(|a| a.id == album.id) {
            self.fav_albums.items.insert(0, album.clone());
            self.fav_albums.total = self.fav_albums.total.saturating_add(1);
            self.fav_albums.selected = self.fav_albums.selected.saturating_add(1);
        }
        self.set_status(
            format!("Added '{}' to albums", album.title),
            StatusLevel::Info,
        );
    }

    fn unfavorite_album(&mut self, album: &Album) {
        let _ = self
            .api_tx
            .send(ApiRequest::UnfavoriteAlbum { album_id: album.id });
        self.set_status(
            format!("Removed '{}' from albums", album.title),
            StatusLevel::Info,
        );
    }

    pub fn toggle_favorite_album(&mut self, album: &Album) {
        if self.fav_albums.items.iter().any(|a| a.id == album.id) {
            self.unfavorite_album(album);
        } else {
            self.favorite_album(album);
        }
    }

    // ── Playlists ─────────────────────────────────────────────────────────────

    fn save_playlist(&mut self, playlist: &Playlist) {
        let _ = self.api_tx.send(ApiRequest::SavePlaylist {
            uuid: playlist.uuid.clone(),
        });
        if !self.playlists.items.iter().any(|p| p.uuid == playlist.uuid) {
            self.playlists.items.insert(0, playlist.clone());
            self.playlists.total = self.playlists.total.saturating_add(1);
            self.playlists.refilter();
        }
        self.set_status(
            format!("Saved '{}' to playlists", playlist.title),
            StatusLevel::Info,
        );
    }

    fn remove_playlist(&mut self, playlist: &Playlist) {
        let _ = self.api_tx.send(ApiRequest::RemovePlaylist {
            uuid: playlist.uuid.clone(),
        });
        self.set_status(
            format!("Removed '{}' from playlists", playlist.title),
            StatusLevel::Info,
        );
    }

    pub fn toggle_save_playlist(&mut self, playlist: &Playlist) {
        if self.playlists.items.iter().any(|p| p.uuid == playlist.uuid) {
            self.remove_playlist(playlist);
        } else {
            self.save_playlist(playlist);
        }
    }

    // ── Radio ─────────────────────────────────────────────────────────────────

    pub fn start_track_radio(&mut self, track: &Track) {
        let _ = self
            .api_tx
            .send(ApiRequest::TrackRadio { track_id: track.id });
        self.set_status(
            format!("Loading radio for '{}'…", track.title),
            StatusLevel::Info,
        );
    }

    pub fn start_artist_radio(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::ArtistRadio {
            artist_id: artist.id,
        });
        self.set_status(
            format!("Loading radio for {}…", artist.name),
            StatusLevel::Info,
        );
    }

    // ── Sort ──────────────────────────────────────────────────────────────────

    /// The sort in effect for the active tab, or `None` on tabs that don't sort.
    ///
    /// An unset field means alphabetical — the same fallback the `sort_*`
    /// helpers use — so this reports what the list is actually ordered by rather
    /// than whether the user has explicitly chosen anything.
    pub fn active_sort(&self) -> Option<SortField> {
        let field = match self.current_tab {
            Tab::Favorites => self.tracks_sort,
            Tab::Artists => self.artists_sort,
            Tab::Albums => self.fav_albums_sort,
            Tab::Playlists => self.playlists_sort,
            Tab::Home | Tab::Search => return None,
        };
        Some(field.unwrap_or(SortField::Alphabetical))
    }

    pub fn open_sort_palette(&mut self) {
        self.sort_palette.active = true;
        // Land on the sort that's already applied, so the palette reflects the
        // current state and Enter re-confirms it instead of silently switching
        // to whichever option happens to be listed first.
        let current = self.active_sort();
        self.sort_palette.selected = SortPalette::get_options(self.current_tab)
            .iter()
            .position(|(_, field)| Some(*field) == current)
            .unwrap_or(0);
    }

    pub fn apply_sort(&mut self, field: SortField) {
        self.sort_palette.active = false;
        match self.current_tab {
            Tab::Home | Tab::Search => {}
            Tab::Favorites => {
                self.tracks_sort = Some(field);
                self.sort_favorites();
            }
            Tab::Artists => {
                self.artists_sort = Some(field);
                self.sort_artists();
            }
            Tab::Albums => {
                self.fav_albums_sort = Some(field);
                self.sort_fav_albums();
            }
            Tab::Playlists => {
                self.playlists_sort = Some(field);
                self.sort_playlists();
            }
        }
    }

    // ── Sorting ──────────────────────────────────────────────────────────────
    //
    // Each list's ordering lives in one place so it can be applied both when the
    // user picks from the sort palette and when fresh data arrives. The response
    // handlers used to inline "sort alphabetically if no sort is set", which
    // meant a sort restored from preferences suppressed the default without ever
    // applying itself — leaving the list in raw API order.
    //
    // `None` means "never chosen", which sorts alphabetically.

    pub(crate) fn sort_favorites(&mut self) {
        match self.tracks_sort.unwrap_or(SortField::Alphabetical) {
            SortField::Alphabetical => self
                .favorites
                .items
                .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
            SortField::LastAdded => self
                .favorites
                .items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            SortField::ByArtist => self.favorites.items.sort_by(|a, b| {
                a.artist_name()
                    .to_lowercase()
                    .cmp(&b.artist_name().to_lowercase())
            }),
        }
        // `matches` holds positions, so reordering invalidates it.
        self.favorites.refilter();
    }

    pub(crate) fn sort_artists(&mut self) {
        match self.artists_sort.unwrap_or(SortField::Alphabetical) {
            SortField::LastAdded => self
                .artists
                .items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            // Artists have no album/artist axis to sort on, so anything else
            // falls back to name order.
            _ => self
                .artists
                .items
                .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        }
        // `matches` holds positions, so reordering invalidates it.
        self.artists.refilter();
    }

    pub(crate) fn sort_fav_albums(&mut self) {
        match self.fav_albums_sort.unwrap_or(SortField::Alphabetical) {
            SortField::Alphabetical => self
                .fav_albums
                .items
                .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
            SortField::LastAdded => self
                .fav_albums
                .items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            SortField::ByArtist => self.fav_albums.items.sort_by(|a, b| {
                a.artist_name()
                    .to_lowercase()
                    .cmp(&b.artist_name().to_lowercase())
            }),
        }
        // `matches` holds positions, so reordering invalidates it.
        self.fav_albums.refilter();
    }

    pub(crate) fn sort_playlists(&mut self) {
        match self.playlists_sort.unwrap_or(SortField::Alphabetical) {
            SortField::LastAdded => self
                .playlists
                .items
                .sort_by(|a, b| b.added_at.cmp(&a.added_at)),
            _ => self
                .playlists
                .items
                .sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        }
        // `matches` holds positions, so reordering invalidates it.
        self.playlists.refilter();
    }

    // ── Filtering ─────────────────────────────────────────────────────────────

    /// Whether the current tab shows a list that can be filtered. Detail views
    /// have their own lists and are not covered.
    pub fn filterable_tab(&self) -> bool {
        self.view_stack.is_empty()
            && matches!(
                self.current_tab,
                Tab::Favorites | Tab::Artists | Tab::Albums | Tab::Playlists
            )
    }

    /// The filter query of the list the current tab shows.
    pub fn active_filter(&self) -> &str {
        match self.current_tab {
            Tab::Favorites => self.favorites.filter(),
            Tab::Artists => self.artists.filter(),
            Tab::Albums => self.fav_albums.filter(),
            Tab::Playlists => self.playlists.filter(),
            _ => "",
        }
    }

    pub fn edit_active_filter(&mut self, edit: impl FnOnce(&mut String)) {
        match self.current_tab {
            Tab::Favorites => self.favorites.edit_filter(edit),
            Tab::Artists => self.artists.edit_filter(edit),
            Tab::Albums => self.fav_albums.edit_filter(edit),
            Tab::Playlists => self.playlists.edit_filter(edit),
            _ => {}
        }
    }

    pub fn clear_active_filter(&mut self) {
        self.edit_active_filter(|f| f.clear());
    }
}
