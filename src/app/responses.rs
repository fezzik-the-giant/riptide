// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::{ApiRequest, ApiResponse};
use crate::api::models::*;
use crate::player::{PlayerCmd, PlayerEvent};
use super::{App, SearchPane, StatusLevel, View};

impl App {
    pub fn handle_api_response(&mut self, resp: ApiResponse) {
        match resp {
            ApiResponse::Artists(items, total) => {
                self.artists.append(items, total);
                if self.artists_sort.is_none() {
                    self.artists.items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                }
            }

            ApiResponse::FavAlbums(items, total) => {
                let existing_ids: std::collections::HashSet<u64> =
                    self.fav_albums.items.iter().map(|a| a.id).collect();
                let unique: Vec<Album> = items.into_iter()
                    .filter(|a| !existing_ids.contains(&a.id))
                    .collect();
                self.fav_albums.append(unique, total);
                if self.fav_albums_sort.is_none() {
                    self.fav_albums.items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                }
                self.load_fav_albums();
            }

            ApiResponse::AlbumFavorited => {}

            ApiResponse::AlbumUnfavorited { album_id } => {
                self.fav_albums.items.retain(|a| a.id != album_id);
                self.fav_albums.total = self.fav_albums.total.saturating_sub(1);
                self.fav_albums.selected = self.fav_albums.selected
                    .min(self.fav_albums.items.len().saturating_sub(1));
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
                if self.playlists_sort.is_none() {
                    self.playlists.items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                }
            }

            ApiResponse::Favorites(items, total) => {
                let was_empty = self.favorites.items.is_empty();
                let existing_ids: std::collections::HashSet<u64> =
                    self.favorites.items.iter().map(|t| t.id).collect();
                let unique: Vec<Track> = items.into_iter()
                    .filter(|t| !existing_ids.contains(&t.id))
                    .collect();
                self.favorites.append(unique, total);
                if self.favorites_sort.is_none() {
                    self.favorites.items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                }
                if was_empty && self.now_playing.track.is_none() {
                    if let Some(first) = self.favorites.items.first().cloned() {
                        self.now_playing.track = Some(first);
                        self.fetch_now_playing_metadata();
                    }
                }
            }

            ApiResponse::ArtistTopTracks { artist_id, tracks } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let n = tracks.len() as u32;
                        let total = detail.tracks.total.max(n);
                        detail.tracks.append(tracks, total);
                        detail.tracks.exhausted = true;
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
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album.id {
                        detail.album = album;
                    }
                }
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
                let is_now_playing = self.now_playing.track.as_ref()
                    .map(|t| t.album.id) == Some(album_id);
                if is_now_playing {
                    self.now_playing.art_bytes = Some(image_data.clone());
                    self.now_playing.art_loading = false;
                }
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album_id {
                        detail.art_bytes = Some(image_data);
                        detail.art_loading = false;
                    }
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

            ApiResponse::ArtistBio { artist_id, text } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        detail.bio = if text.is_empty() { None } else { Some(text) };
                        detail.bio_loading = false;
                    }
                }
            }

            ApiResponse::PlaylistTracks { uuid, tracks, total } => {
                // 1. Update the detail view while it's open.
                if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.playlist.uuid == uuid {
                        detail.tracks.append(tracks.clone(), total);
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

                    // If the detail view is gone, keep firing page requests ourselves.
                    let detail_open = if let Some(View::PlaylistDetail(d)) = self.view_stack.last() {
                        d.playlist.uuid == uuid
                    } else {
                        false
                    };
                    if !detail_open && self.now_playing.source_playlist_next_offset < total {
                        let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks {
                            uuid,
                            offset: self.now_playing.source_playlist_next_offset,
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

            ApiResponse::SearchResults(results) => {
                self.search.loading = false;
                self.search.tracks   = results.tracks.map(|p| p.items).unwrap_or_default();
                self.search.artists  = results.artists.map(|p| p.items).unwrap_or_default();
                self.search.playlists = results.playlists.map(|p| p.items).unwrap_or_default();
                self.search.track_sel = 0;
                self.search.artist_sel = 0;
                self.search.playlist_sel = 0;
                self.search.reset_viewports();
                self.search.pane = SearchPane::Tracks;
            }

            ApiResponse::StreamUrl { track_id, url } => {
                let idx = self.now_playing.queue_index;
                if self.now_playing.queue.get(idx).map(|t| t.id) == Some(track_id) {
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

            ApiResponse::FavoriteAdded | ApiResponse::ArtistFollowed => {}

            ApiResponse::FavoriteRemoved { track_id } => {
                self.favorites.items.retain(|t| t.id != track_id);
                self.favorites.total = self.favorites.total.saturating_sub(1);
                self.favorites.selected = self.favorites.selected
                    .min(self.favorites.items.len().saturating_sub(1));
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
                self.set_status(msg.clone(), StatusLevel::Error);
                // Also set error on home sections if they're loading
                if self.home_new_releases.loading {
                    self.home_new_releases.error = Some(msg.clone());
                    self.home_new_releases.loading = false;
                }
                if self.home_daily_mixes.loading {
                    self.home_daily_mixes.error = Some(msg.clone());
                    self.home_daily_mixes.loading = false;
                }
                if self.home_discovery_mixes.loading {
                    self.home_discovery_mixes.error = Some(msg);
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
