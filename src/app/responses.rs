// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::{ApiRequest, ApiResponse};
use crate::api::models::*;
use crate::player::{PlayerCmd, PlayerEvent};
use super::{App, StatusLevel, View};

impl App {
    pub fn handle_api_response(&mut self, resp: ApiResponse) {
        match resp {
            ApiResponse::Artists(items, total) => {
                self.artists.append(items, total);
                self.artists.exhausted = true;
                self.sort_artists();
                self.rebuild_favorite_artist_ids();
            }

            ApiResponse::FavAlbumsPage { albums, total, next_url } => {
                let existing_ids: std::collections::HashSet<u64> =
                    self.fav_albums.items.iter().map(|a| a.id).collect();
                let unique: Vec<Album> = albums.into_iter()
                    .filter(|a| !existing_ids.contains(&a.id))
                    .collect();
                self.fav_albums.append(unique, total);
                let has_next = next_url.is_some();
                self.fav_albums.pagination_cursor = next_url;
                if !has_next {
                    self.fav_albums.exhausted = true;
                }
                self.sort_fav_albums();
                self.rebuild_favorite_album_ids();
                if !self.fav_albums.exhausted {
                    self.load_fav_albums();
                }
            }

            ApiResponse::AlbumFavorited { album_id } => {
                self.favorite_album_ids.insert(album_id);
                self.fav_albums.items.clear();
                self.fav_albums.pagination_cursor = None;
                self.fav_albums.exhausted = false;
                self.load_fav_albums();
            }

            ApiResponse::AlbumUnfavorited { album_id } => {
                self.fav_albums.items.retain(|a| a.id != album_id);
                self.fav_albums.total = self.fav_albums.total.saturating_sub(1);
                self.fav_albums.selected = self.fav_albums.selected
                    .min(self.fav_albums.items.len().saturating_sub(1));
                self.favorite_album_ids.remove(&album_id);
            }

            ApiResponse::PlaylistSaved => {}

            ApiResponse::PlaylistRemoved { uuid } => {
                self.playlists.items.retain(|p| p.uuid != uuid);
                self.playlists.total = self.playlists.total.saturating_sub(1);
                self.playlists.selected = self.playlists.selected
                    .min(self.playlists.items.len().saturating_sub(1));
            }

            ApiResponse::Playlists(items, total) => {
                self.playlists.append(items, total);
                self.sort_playlists();
            }

            ApiResponse::Favorites(items, total) => {
                self.favorites.append(items, total);
                self.sort_favorites();
                if self.favorites.should_load_more() {
                    self.load_favorites();
                }
                self.rebuild_favorite_track_ids();
            }

            ApiResponse::ArtistTopTracks { artist_id, tracks } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let n = tracks.len() as u32;
                        let total = detail.tracks.total.max(n);
                        detail.tracks.append(tracks, total);
                        detail.tracks.exhausted = true;

                        if detail.albums.items.is_empty() && !detail.albums.loading {
                            let artist_id = detail.artist.id;
                            detail.albums.loading = true;
                            let _ = self.api_tx.send(ApiRequest::LoadArtistAlbums { artist_id });
                        }
                        if detail.eps.items.is_empty() && !detail.eps.loading {
                            let artist_id = detail.artist.id;
                            detail.eps.loading = true;
                            let _ = self.api_tx.send(ApiRequest::LoadArtistEPs { artist_id });
                        }
                        if detail.singles.items.is_empty() && !detail.singles.loading {
                            let artist_id = detail.artist.id;
                            detail.singles.loading = true;
                            let _ = self.api_tx.send(ApiRequest::LoadArtistSingles { artist_id });
                        }
                    }
                }
            }

            ApiResponse::ArtistAlbums { artist_id, albums } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let n = albums.len() as u32;
                        let total = detail.albums.total.max(n);
                        detail.albums.append(albums, total);
                        detail.albums.items.sort_by(|a, b| {
                            b.release_date.as_deref().cmp(&a.release_date.as_deref())
                        });
                        detail.albums.exhausted = true;
                    }
                }
            }

            ApiResponse::ArtistEPs { artist_id, albums } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let eps: Vec<Album> = albums.into_iter()
                            .filter(|a| a.number_of_tracks.unwrap_or(0) >= 3)
                            .collect();
                        let n = eps.len() as u32;
                        let total = detail.eps.total.max(n);
                        detail.eps.append(eps, total);
                        detail.eps.items.sort_by(|a, b| {
                            b.release_date.as_deref().cmp(&a.release_date.as_deref())
                        });
                        detail.eps.exhausted = true;
                    }
                }
            }

            ApiResponse::ArtistSingles { artist_id, albums } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let singles: Vec<Album> = albums.into_iter()
                            .filter(|a| a.number_of_tracks.unwrap_or(0) <= 2)
                            .collect();
                        let n = singles.len() as u32;
                        let total = detail.singles.total.max(n);
                        detail.singles.append(singles, total);
                        detail.singles.items.sort_by(|a, b| {
                            b.release_date.as_deref().cmp(&a.release_date.as_deref())
                        });
                        detail.singles.exhausted = true;
                    }
                }
            }

            ApiResponse::AlbumLoaded { album } => {
                let album_id = album.id;
                let cover = album.cover.clone();
                tracing::debug!("AlbumLoaded album_id={}, cover={:?}", album_id, cover);

                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album.id {
                        detail.album = album.clone();
                        if let Some(cover_url) = cover.clone() {
                            detail.art_loading = true;
                            let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id: cover_url });
                        }
                    }
                }

                // Also handle when album is loaded for now_playing track (from fetch_now_playing_art)
                let is_now_playing = self.now_playing.is_current_album(album_id);
                if is_now_playing {
                    if let Some(cover_url) = cover {
                        if self.now_playing.presentation_art_discovering_cover() {
                            self.now_playing.finish_presentation_art_load();
                        }
                        self.now_playing.art_source = Some(cover_url.clone());
                        self.now_playing.art_loading = true;
                        let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id: cover_url });
                        if self.art_fullscreen {
                            self.fetch_presentation_art();
                        }
                    } else if self.now_playing.art_source.is_none() {
                        self.now_playing.art_loading = false;
                        self.now_playing.finish_presentation_art_load();
                    }
                }
            }

            ApiResponse::AlbumLoadFailed { album_id, error } => {
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut()
                    && detail.album.id == album_id
                {
                    detail.art_loading = false;
                }
                if self.now_playing.is_current_album(album_id) {
                    self.now_playing.art_loading = false;
                    if self.now_playing.presentation_art_discovering_cover() {
                        self.now_playing.finish_presentation_art_load();
                    }
                }
                self.set_status(format!("album: {error}"), StatusLevel::Error);
            }

            ApiResponse::AlbumTracks { album_id, tracks } => {
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album_id {
                        let n = tracks.len() as u32;
                        detail.tracks.append(tracks, n);
                        detail.tracks.exhausted = true;
                    }
                }
            }

            ApiResponse::AlbumArt { album_id, image_data } => {
                tracing::debug!("AlbumArt response for album_id={}, bytes={}", album_id, image_data.len());
                let is_now_playing = self.now_playing.is_current_album(album_id);
                tracing::debug!("is_now_playing={}", is_now_playing);
                if is_now_playing {
                    tracing::debug!("Setting now_playing.art_bytes");
                    self.now_playing.set_art_bytes(Some(image_data.clone()));
                    self.now_playing.art_loading = false;
                }
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album_id {
                        detail.art_bytes = Some(image_data);
                        detail.art_loading = false;
                    }
                }
            }

            ApiResponse::AlbumArtFailed { album_id, error } => {
                if self.now_playing.is_current_album(album_id) {
                    self.now_playing.art_loading = false;
                }
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut()
                    && detail.album.id == album_id
                {
                    detail.art_loading = false;
                }
                self.set_status(format!("album art: {error}"), StatusLevel::Error);
            }

            ApiResponse::PresentationArt { album_id, image_data } => {
                let is_now_playing = self.now_playing.is_current_album(album_id);
                tracing::debug!(
                    album_id,
                    bytes = image_data.as_ref().map_or(0, Vec::len),
                    is_now_playing,
                    "received presentation artwork"
                );
                if is_now_playing {
                    self.now_playing.set_presentation_art_bytes(image_data);
                    self.now_playing.finish_presentation_art_load();
                }
            }

            ApiResponse::ArtistArt { artist_id, image_data } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        detail.art_bytes = Some(image_data);
                        detail.art_loading = false;
                    }
                }
            }

            ApiResponse::PlaylistArt { uuid, image_data } => {
                if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.playlist.uuid == uuid {
                        detail.art_bytes = Some(image_data);
                        detail.art_loading = false;
                    }
                }
            }

            ApiResponse::ArtistBio { artist_id, text } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        detail.bio = if text.is_empty() { None } else { Some(text) };
                        detail.bio_loading = false;
                    }
                }
            }

            ApiResponse::ArtistPicture { artist_id, picture_url } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        if let Some(url) = picture_url {
                            detail.art_loading = true;
                            let _ = self.api_tx.send(ApiRequest::FetchArtistArt { artist_id, picture_id: url });
                        }
                    }
                }
            }

            ApiResponse::PlaylistTracks { uuid, tracks, total, next_cursor, description, cover } => {
                // 1. Update the detail view while it's open.
                if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.playlist.uuid == uuid {
                        tracing::debug!("PlaylistTracks response: has_description={}, has_cover={}, total_tracks={}", description.is_some(), cover.is_some(), total);
                        if let Some(desc) = description {
                            tracing::debug!("Setting description: {}", desc);
                            detail.playlist.description = Some(desc);
                        }
                        // Only the initial /playlists/{uuid} request carries the
                        // playlist's item count. Later pages come from the
                        // relationship endpoint, which has no attributes block
                        // and reports 0 — don't let that clobber a real count.
                        if total > 0 {
                            detail.playlist.number_of_tracks = Some(total);
                        }
                        if let Some(cov_url) = cover {
                            tracing::debug!("Setting cover: {}", cov_url);
                            detail.playlist.cover = Some(cov_url.clone());
                            detail.art_loading = true;
                            let _ = self.api_tx.send(ApiRequest::FetchPlaylistArt {
                                uuid: uuid.clone(),
                                cover_url: cov_url,
                            });
                        }
                        detail.tracks.append(tracks.clone(), total);
                        detail.tracks.pagination_cursor = next_cursor.clone();
                        detail.tracks.exhausted = next_cursor.is_none();
                        match &next_cursor {
                            Some(c) => tracing::debug!("Stored cursor: {} | Total items loaded: {}", c, detail.tracks.items.len()),
                            None => tracing::debug!("No more pages | Total items loaded: {}", detail.tracks.items.len()),
                        }
                    }
                }

                // 2. Eagerly request the next page (no waiting for the user to scroll).
                self.load_more_playlist_tracks();

                // 3. Extend the live queue if we're playing from this playlist.
                let is_source = self.now_playing.source_playlist_uuid.as_deref() == Some(&uuid);
                if is_source {
                    let qi = self.now_playing.queue_index;
                    let old_queue_len = self.now_playing.queue.len();

                    if self.now_playing.shuffle {
                        self.now_playing.original_queue.extend(tracks.clone());
                        use rand::Rng;
                        let mut rng = rand::thread_rng();
                        for track in tracks {
                            let pos = if self.now_playing.queue.len() > qi + 1 {
                                rng.gen_range(qi + 1..=self.now_playing.queue.len())
                            } else {
                                self.now_playing.queue.len()
                            };
                            self.now_playing.queue.insert(pos, track);
                        }
                        self.now_playing.source_playlist_next_offset =
                            self.now_playing.original_queue.len() as u32;
                    } else {
                        self.now_playing.queue.extend(tracks);
                        self.now_playing.source_playlist_next_offset =
                            self.now_playing.queue.len() as u32;
                    }

                    self.now_playing.source_playlist_cursor = next_cursor.clone();

                    // If the detail view is gone, keep firing page requests ourselves (if more pages available).
                    let detail_open = if let Some(View::PlaylistDetail(d)) = self.view_stack.last() {
                        d.playlist.uuid == uuid
                    } else {
                        false
                    };
                    if !detail_open && next_cursor.is_some() && self.now_playing.source_playlist_next_offset < total {
                        let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks {
                            uuid,
                            next_url: self.now_playing.source_playlist_cursor.clone(),
                        });
                    }

                    if old_queue_len <= qi + 1 {
                        if let Some(next) = self.now_playing.queue.get(qi + 1) {
                            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl {
                                track_id: next.id,
                            });
                        }
                    }
                }
            }

            ApiResponse::SearchTracks(page) => {
                if self.search.tracks.is_empty() {
                    // Initial search response
                    self.search.tracks = page.tracks;
                    if let Some(next_url) = &page.next_url {
                        self.search.tracks_awaiting_page2 = true;
                        let _ = self.api_tx.send(ApiRequest::SearchTracksNext {
                            next_url: next_url.clone()
                        });
                    } else {
                        self.search.tracks_awaiting_page2 = false;
                        if !self.search.artists_awaiting_page2 && !self.search.playlists_awaiting_page2 {
                            self.search.loading = false;
                        }
                    }
                } else {
                    // Pagination response - append
                    self.search.tracks.extend(page.tracks);
                    if let Some(next_url) = &page.next_url {
                        let _ = self.api_tx.send(ApiRequest::SearchTracksNext {
                            next_url: next_url.clone()
                        });
                    } else {
                        self.search.tracks_awaiting_page2 = false;
                        if !self.search.artists_awaiting_page2 && !self.search.playlists_awaiting_page2 {
                            self.search.loading = false;
                        }
                    }
                }
                self.search.tracks_next_url = page.next_url;
            }

            ApiResponse::SearchArtistsResults(page) => {
                if self.search.artists.is_empty() {
                    // Initial search response
                    self.search.artists = page.artists;
                    if let Some(next_url) = &page.next_url {
                        self.search.artists_awaiting_page2 = true;
                        let _ = self.api_tx.send(ApiRequest::SearchArtistsNext {
                            next_url: next_url.clone()
                        });
                    } else {
                        self.search.artists_awaiting_page2 = false;
                        if !self.search.tracks_awaiting_page2 && !self.search.playlists_awaiting_page2 {
                            self.search.loading = false;
                        }
                    }
                } else {
                    // Pagination response - append
                    self.search.artists.extend(page.artists);
                    if let Some(next_url) = &page.next_url {
                        let _ = self.api_tx.send(ApiRequest::SearchArtistsNext {
                            next_url: next_url.clone()
                        });
                    } else {
                        self.search.artists_awaiting_page2 = false;
                        if !self.search.tracks_awaiting_page2 && !self.search.playlists_awaiting_page2 {
                            self.search.loading = false;
                        }
                    }
                }
                self.search.artists_next_url = page.next_url;
            }

            ApiResponse::SearchPlaylistsResults(page) => {
                if self.search.playlists.is_empty() {
                    // Initial search response
                    self.search.playlists = page.playlists;
                    if let Some(next_url) = &page.next_url {
                        self.search.playlists_awaiting_page2 = true;
                        let _ = self.api_tx.send(ApiRequest::SearchPlaylistsNext {
                            next_url: next_url.clone()
                        });
                    } else {
                        self.search.playlists_awaiting_page2 = false;
                        if !self.search.tracks_awaiting_page2 && !self.search.artists_awaiting_page2 {
                            self.search.loading = false;
                        }
                    }
                } else {
                    // Pagination response - append
                    self.search.playlists.extend(page.playlists);
                    if let Some(next_url) = &page.next_url {
                        let _ = self.api_tx.send(ApiRequest::SearchPlaylistsNext {
                            next_url: next_url.clone()
                        });
                    } else {
                        self.search.playlists_awaiting_page2 = false;
                        if !self.search.tracks_awaiting_page2 && !self.search.artists_awaiting_page2 {
                            self.search.loading = false;
                        }
                    }
                }
                self.search.playlists_next_url = page.next_url;
            }


            ApiResponse::StreamUrl { track_id, url } => {
                let idx = self.now_playing.queue_index;
                if self.now_playing.queue.get(idx).map(|t| t.id) == Some(track_id) {
                    // Always update the track when we get a successful stream URL for the current track
                    let track_changed = self.now_playing.track.as_ref().map(|t| t.id) != Some(track_id);
                    self.now_playing.track = self.now_playing.queue.get(idx).cloned();

                    if track_changed {
                        // Clear old art and lyrics so we don't show the previous track's content
                        tracing::debug!("Clearing old art and lyrics for new track");
                        self.now_playing.set_art_bytes(None);
                        self.now_playing.art_loading = true;
                        self.now_playing.lyrics_synced.clear();
                        self.now_playing.lyrics_plain.clear();
                        self.now_playing.lyrics_loading = true;
                    }

                    // Fetch track details, art, and lyrics now that playback is confirmed
                    let _ = self.api_tx.send(ApiRequest::GetTrackDetails { track_id });
                    self.fetch_now_playing_metadata();
                    self.push_mpris_state();

                    let _ = self.player_tx.send(PlayerCmd::Play(url));
                    if let Some(next) = self.now_playing.queue.get(idx + 1) {
                        let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
                    }
                } else if self.now_playing.queue.get(idx + 1).map(|t| t.id) == Some(track_id) {
                    let _ = self.player_tx.send(PlayerCmd::Append(url));
                }
            }

            ApiResponse::Lyrics { track_id, synced, plain } => {
                if self.now_playing.track.as_ref().map(|t| t.id) == Some(track_id) {
                    self.now_playing.lyrics_synced = synced;
                    self.now_playing.lyrics_plain = plain;
                    self.now_playing.lyrics_loading = false;
                }
            }

            ApiResponse::TrackDetails { track_id, track, cover_url } => {
                tracing::debug!("TrackDetails for track_id={}, cover_url={:?}", track_id, cover_url);
                if self.now_playing.track.as_ref().map(|t| t.id) == Some(track_id) {
                    self.now_playing.track = Some(track);
                    // Only fetch track cover if it's a valid image (not video)
                    if let Some(url) = cover_url {
                        if url.ends_with(".jpg") || url.ends_with(".png") || url.ends_with(".jpeg") {
                            tracing::debug!("TrackDetails has valid image cover_url, fetching");
                            self.now_playing.art_source = Some(url.clone());
                            self.now_playing.art_loading = true;
                            self.now_playing.set_art_bytes(None);
                            let _ = self.api_tx.send(ApiRequest::FetchTrackArt { track_id, cover_url: url });
                            if self.art_fullscreen {
                                self.fetch_presentation_art();
                            }
                        } else {
                            tracing::debug!("TrackDetails has non-image cover_url ({}), skipping", url);
                        }
                    }
                }
            }

            ApiResponse::TrackArt { track_id, image_data } => {
                if self.now_playing.track.as_ref().map(|t| t.id) == Some(track_id) {
                    self.now_playing.set_art_bytes(Some(image_data));
                    self.now_playing.art_loading = false;
                }
            }

            ApiResponse::TrackArtFailed { track_id, error } => {
                if self.now_playing.track.as_ref().map(|track| track.id) == Some(track_id) {
                    self.now_playing.art_loading = false;
                }
                self.set_status(format!("track art: {error}"), StatusLevel::Error);
            }

            ApiResponse::FavoriteAdded | ApiResponse::ArtistFollowed => {}

            ApiResponse::FavoriteRemoved { track_id } => {
                self.favorites.items.retain(|t| t.id != track_id);
                self.favorites.total = self.favorites.total.saturating_sub(1);
                self.favorites.selected = self.favorites.selected
                    .min(self.favorites.items.len().saturating_sub(1));
                self.rebuild_favorite_track_ids();
            }

            ApiResponse::ArtistUnfollowed { artist_id } => {
                self.artists.items.retain(|a| a.id != artist_id);
                self.artists.total = self.artists.total.saturating_sub(1);
                self.artists.selected = self.artists.selected
                    .min(self.artists.items.len().saturating_sub(1));
            }

            ApiResponse::RadioTracks { tracks } => {
                if tracks.is_empty() {
                    self.set_status("No radio tracks available".to_string(), StatusLevel::Error);
                } else {
                    self.play_tracks(tracks, 0);
                }
            }

            ApiResponse::SearchedArtists(artists) => {
                if let Some(search_query) = self.artist_selection.searching_for.take() {
                    let exact_match: Option<Artist> = artists.into_iter()
                        .find(|a| a.name.to_lowercase() == search_query.to_lowercase());

                    if let Some(artist) = exact_match {
                        self.open_artist(artist);
                    } else {
                        self.set_status(format!("Artist '{}' not found", search_query), StatusLevel::Error);
                    }
                }
            }

            ApiResponse::NewReleases(items) => {
                self.home_new_releases.items = items;
                self.home_new_releases.loading = false;
            }

            ApiResponse::DailyMixes(items) => {
                self.home_daily_mixes.items = items;
                self.home_daily_mixes.loading = false;
            }

            ApiResponse::DiscoveryMixes(items) => {
                self.home_discovery_mixes.items = items;
                self.home_discovery_mixes.loading = false;
            }

            ApiResponse::Error(msg) => {
                let display_msg = if msg.contains("no stream URL available for track") {
                    // Try to enhance the error message with track name
                    if let Some(track_id_str) = msg.split("track ").last() {
                        if let Ok(track_id) = track_id_str.parse::<u64>() {
                            // Look for this track in the queue
                            if let Some(track) = self.now_playing.queue.iter().find(|t| t.id == track_id) {
                                format!("No stream available for \"{}\"", track.title)
                            } else {
                                msg.clone()
                            }
                        } else {
                            msg.clone()
                        }
                    } else {
                        msg.clone()
                    }
                } else {
                    msg.clone()
                };

                self.set_status(display_msg.clone(), StatusLevel::Error);
                // Also set error on home sections if they're loading
                if self.home_new_releases.loading {
                    self.home_new_releases.error = Some(display_msg.clone());
                    self.home_new_releases.loading = false;
                }
                if self.home_daily_mixes.loading {
                    self.home_daily_mixes.error = Some(display_msg.clone());
                    self.home_daily_mixes.loading = false;
                }
                if self.home_discovery_mixes.loading {
                    self.home_discovery_mixes.error = Some(display_msg);
                    self.home_discovery_mixes.loading = false;
                }
            }
        }
    }

    pub fn handle_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::TrackStarted => {
                self.now_playing.position = 0.0;
                self.now_playing.duration = 0.0;
                self.now_playing.active = true;
                self.now_playing.paused = false;
                self.now_playing.sample_rate = None;
                self.now_playing.codec = None;
                self.now_playing.lastfm_sent = false;
                if let Some(track) = &self.now_playing.track {
                    let title = format!("{} — {}", track.artist_name(), track.title);
                    let _ = self.player_tx.send(PlayerCmd::SetMediaTitle(title));
                }
                self.push_mpris_state();
            }
            PlayerEvent::TrackEnded => {
                if self.now_playing.queue_index + 1 < self.now_playing.queue.len() {
                    self.now_playing.queue_index += 1;
                    self.now_playing.track =
                        self.now_playing.queue.get(self.now_playing.queue_index).cloned();
                    let next_idx = self.now_playing.queue_index + 1;
                    if let Some(next) = self.now_playing.queue.get(next_idx) {
                        let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
                    }
                    if let Some(current) = self.now_playing.queue.get(self.now_playing.queue_index) {
                        let _ = self.api_tx.send(ApiRequest::GetTrackDetails { track_id: current.id });
                    }
                    self.fetch_now_playing_metadata();
                } else {
                    self.now_playing.active = false;
                    self.push_mpris_state();
                }
                self.now_playing.position = 0.0;
            }
            PlayerEvent::Position(p)  => {
                if !self.now_playing.active { return; }
                // Only accept position updates that move forward (with 10ms tolerance for jitter).
                // This prevents the audio widget from showing position going backward.
                if p >= self.now_playing.position - 0.01 {
                    self.now_playing.position = p;
                    self.push_mpris_state();
                }
            }
            PlayerEvent::Duration(d)  => {
                if !self.now_playing.active { return; }
                self.now_playing.duration = d;
                // Send track to Last.fm once when we first learn the duration
                if !self.now_playing.lastfm_sent && d > 0.0 {
                    if let Some(track) = &self.now_playing.track {
                        let artist_names = if !track.artists.is_empty() {
                            track.artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ")
                        } else if let Some(ref artist) = track.artist {
                            artist.name.clone()
                        } else {
                            "Unknown".to_string()
                        };
                        let album = Some(track.album.title.clone());

                        let _ = self.lastfm_tx.send(crate::lastfm::LastfmCmd::UpdatePlayingTrack {
                            track_id: track.id,
                            artist: artist_names,
                            track_name: track.title.clone(),
                            album,
                            duration: d,
                        });
                        self.now_playing.lastfm_sent = true;
                    }
                }
            }
            PlayerEvent::Paused(p)    => {
                // Only send pause/resume command if state actually changed
                if self.now_playing.paused != p {
                    if p {
                        let _ = self.lastfm_tx.send(crate::lastfm::LastfmCmd::Pause);
                    } else {
                        let _ = self.lastfm_tx.send(crate::lastfm::LastfmCmd::Resume);
                    }
                }
                self.now_playing.paused = p;
                self.push_mpris_state();
            }
            PlayerEvent::SampleRate(r) => { self.now_playing.sample_rate = Some(r); }
            PlayerEvent::Codec(c)     => { self.now_playing.codec = Some(c); }
            PlayerEvent::Error(e)     => {
                self.set_status(format!("Player: {e}"), StatusLevel::Error);
            }
            PlayerEvent::CurrVolume(v) => { self.now_playing.volume = v}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;

    fn track(album_id: u64) -> Track {
        Track {
            id: 1,
            title: "Track".to_string(),
            duration: 180,
            artist: None,
            artists: Vec::new(),
            album: Album {
                id: album_id,
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
    fn stale_presentation_art_does_not_replace_the_current_request() {
        let mut app = test_app().0;
        app.now_playing.track = Some(track(2));
        app.now_playing.begin_presentation_art_fetch();

        app.handle_api_response(ApiResponse::PresentationArt {
            album_id: 99,
            image_data: Some(vec![9, 9, 9]),
        });

        assert!(app.now_playing.presentation_art_loading());
        assert!(app.now_playing.presentation_art_bytes().is_none());

        app.handle_api_response(ApiResponse::PresentationArt {
            album_id: 2,
            image_data: Some(vec![1, 2, 3]),
        });
        assert!(!app.now_playing.presentation_art_loading());
        assert_eq!(app.now_playing.presentation_art_bytes(), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn album_without_a_cover_finishes_art_loading() {
        let mut app = test_app().0;
        app.now_playing.track = Some(track(2));
        app.now_playing.art_loading = true;
        app.now_playing.begin_presentation_art_discovery();

        app.handle_api_response(ApiResponse::AlbumLoaded { album: track(2).album });

        assert!(!app.now_playing.art_loading);
        assert!(!app.now_playing.presentation_art_loading());
    }

    #[test]
    fn unrelated_album_refresh_does_not_cancel_a_known_art_request() {
        let mut app = test_app().0;
        app.now_playing.track = Some(track(2));
        app.now_playing.art_source = Some("cover-id".to_string());
        app.now_playing.begin_presentation_art_fetch();

        app.handle_api_response(ApiResponse::AlbumLoaded { album: track(2).album });

        assert!(app.now_playing.presentation_art_loading());
    }

    #[test]
    fn discovered_cover_starts_the_presentation_fetch() {
        let (mut app, mut api_rx) = test_app();
        while api_rx.try_recv().is_ok() {}
        app.now_playing.track = Some(track(2));
        app.now_playing.begin_presentation_art_discovery();
        app.art_fullscreen = true;
        let mut album = track(2).album;
        album.cover = Some("cover-id".to_string());

        app.handle_api_response(ApiResponse::AlbumLoaded { album });

        assert!(app.now_playing.presentation_art_loading());
        assert!(matches!(
            api_rx.try_recv(),
            Ok(ApiRequest::FetchAlbumArt { album_id: 2, cover_id })
                if cover_id == "cover-id"
        ));
        assert!(matches!(
            api_rx.try_recv(),
            Ok(ApiRequest::FetchPresentationArt { album_id: 2, cover_id })
                if cover_id == "cover-id"
        ));
    }

    #[test]
    fn album_lookup_failure_finishes_discovery() {
        let mut app = test_app().0;
        app.now_playing.track = Some(track(2));
        app.now_playing.art_loading = true;
        app.now_playing.begin_presentation_art_discovery();

        app.handle_api_response(ApiResponse::AlbumLoadFailed {
            album_id: 2,
            error: "offline".to_string(),
        });

        assert!(!app.now_playing.art_loading);
        assert!(!app.now_playing.presentation_art_loading());
        assert!(matches!(
            &app.status,
            Some((message, StatusLevel::Error, _)) if message == "album: offline"
        ));
    }

    #[test]
    fn unavailable_presentation_art_finishes_loading() {
        let mut app = test_app().0;
        app.now_playing.track = Some(track(2));
        app.now_playing.begin_presentation_art_fetch();

        app.handle_api_response(ApiResponse::PresentationArt {
            album_id: 2,
            image_data: None,
        });

        assert!(!app.now_playing.presentation_art_loading());
        assert!(app.now_playing.presentation_art_bytes().is_none());
    }
}
