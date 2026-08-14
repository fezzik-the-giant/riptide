// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! The async API worker.
//!
//! Owns the [`ApiClient`] and turns [`ApiRequest`]s from the TUI into
//! [`ApiResponse`]s, one request at a time off an unbounded channel.

use std::sync::Arc;
use tokio::sync::mpsc;

use super::client::ApiClient;
use super::messages::{ApiRequest, ApiResponse};
use super::models::*;

// ── Worker ────────────────────────────────────────────────────────────────────

pub struct ApiWorker {
    client: Arc<ApiClient>,
    rx: mpsc::UnboundedReceiver<ApiRequest>,
    tx: mpsc::UnboundedSender<ApiResponse>,
}

impl ApiWorker {
    pub fn new(
        config: Config,
        rx: mpsc::UnboundedReceiver<ApiRequest>,
        tx: mpsc::UnboundedSender<ApiResponse>,
    ) -> Self {
        Self {
            client: Arc::new(ApiClient::new(config)),
            rx,
            tx,
        }
    }

    pub async fn run(mut self) {
        while let Some(req) = self.rx.recv().await {
            let client = Arc::clone(&self.client);
            let tx = self.tx.clone();

            tokio::spawn(async move {
                let resp = handle_request(client, req).await;
                let _ = tx.send(resp);
            });
        }
    }
}

async fn handle_request(client: Arc<ApiClient>, req: ApiRequest) -> ApiResponse {
    match req {
        ApiRequest::LoadArtists => match client.get_favorite_artists_v2().await {
            Ok((artists, total)) => {
                ApiResponse::Artists(artists, total)
            }
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::LoadPlaylists { offset } => {
            if offset != 0 {
                // v2 API returns all playlists in a single request, so pagination is not supported.
                return ApiResponse::Error("Playlist pagination not supported (v2 API returns all at once)".to_string());
            }

            match client.get_favorite_playlists().await {
                Err(e) => ApiResponse::Error(e.to_string()),
                Ok(fav_page) => {
                    let mut playlists: Vec<Playlist> = fav_page.items.into_iter().map(|entry| {
                        let mut pl = entry.playlist;
                        pl.added_at = entry.created;
                        pl
                    }).collect();

                    // v2 collection — covers playlists saved via the Tidal web/mobile apps.
                    if let Ok((coll, _)) = client.get_user_collection_playlists(None).await {
                        for pl in coll {
                            if !playlists.iter().any(|p| p.uuid == pl.uuid) {
                                playlists.push(pl);
                            }
                        }
                    }

                    let total = playlists.len() as u32;
                    ApiResponse::Playlists(playlists, total)
                }
            }
        }

        ApiRequest::LoadFavAlbums { next_url } => match client.get_favorite_albums(next_url).await {
            Ok((albums, total, next_cursor)) => {
                ApiResponse::FavAlbumsPage { albums, total, next_url: next_cursor }
            }
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::LoadFavorites => match client.get_favorite_tracks().await {
            Ok((tracks, total)) => ApiResponse::Favorites(tracks, total),
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::LoadArtistTopTracks { artist_id } => {
            match client.get_artist_top_tracks(artist_id, 20).await {
                Ok(page) => ApiResponse::ArtistTopTracks { artist_id, tracks: page.items },
                Err(e) => ApiResponse::Error(format!("top tracks: {e}")),
            }
        }

        ApiRequest::LoadArtistAlbums { artist_id } => {
            match client.get_artist_albums(artist_id, 30).await {
                Ok(page) => ApiResponse::ArtistAlbums { artist_id, albums: page.items },
                Err(e) => ApiResponse::Error(format!("albums: {e}")),
            }
        }

        ApiRequest::LoadArtistEPs { artist_id } => {
            match client.get_artist_eps(artist_id, 30).await {
                Ok(page) => ApiResponse::ArtistEPs { artist_id, albums: page.items },
                Err(e) => ApiResponse::Error(format!("EPs: {e}")),
            }
        }

        ApiRequest::LoadArtistSingles { artist_id } => {
            match client.get_artist_singles(artist_id, 30).await {
                Ok(page) => ApiResponse::ArtistSingles { artist_id, albums: page.items },
                Err(e) => ApiResponse::Error(format!("singles: {e}")),
            }
        }

        ApiRequest::LoadArtistBio { artist_id } => {
            // A missing bio (404 or empty) is not an error — return empty string.
            let raw = match client.get_artist_bio(artist_id).await {
                Ok(bio_text) => bio_text,
                Err(_) => String::new(),
            };
            let text = strip_wimplinks(&raw);
            ApiResponse::ArtistBio { artist_id, text }
        }

        ApiRequest::LoadArtistPicture { artist_id } => {
            match client.get_artist_picture(artist_id).await {
                Ok(picture_url) => ApiResponse::ArtistPicture { artist_id, picture_url },
                Err(e) => ApiResponse::Error(format!("artist picture: {e}")),
            }
        }

        ApiRequest::LoadAlbum { album_id } => {
            match client.get_album(album_id).await {
                Ok((album, _cover_url)) => ApiResponse::AlbumLoaded { album },
                Err(error) => ApiResponse::AlbumLoadFailed {
                    album_id,
                    error: error.to_string(),
                },
            }
        }

        ApiRequest::LoadAlbumTracks { album_id } => {
            match client.get_album_tracks(album_id).await {
                Ok(page) => ApiResponse::AlbumTracks { album_id, tracks: page.items },
                Err(e) => ApiResponse::Error(format!("album tracks: {e}")),
            }
        }

        ApiRequest::FetchAlbumArt { album_id, cover_id } => {
            let url = thumbnail_art_url(&cover_id);
            match client.fetch_bytes(&url).await {
                Ok(data) => ApiResponse::AlbumArt { album_id, image_data: data },
                Err(error) => ApiResponse::AlbumArtFailed {
                    album_id,
                    error: error.to_string(),
                },
            }
        }

        ApiRequest::FetchPresentationArt { album_id, cover_id } => {
            let url = presentation_art_url(&cover_id);
            tracing::debug!(album_id, %url, "fetching presentation artwork");
            let image_data = match client.fetch_bytes(&url).await {
                Ok(data) => {
                    tracing::debug!(album_id, bytes = data.len(), "fetched presentation artwork");
                    Some(data)
                }
                Err(error) => {
                    tracing::warn!("presentation art unavailable for album {album_id}: {error}");
                    None
                }
            };
            ApiResponse::PresentationArt { album_id, image_data }
        }

        ApiRequest::FetchArtistArt { artist_id, picture_id } => {
            let url = thumbnail_art_url(&picture_id);
            tracing::debug!("FetchArtistArt for artist {}: {}", artist_id, url);
            match client.fetch_bytes(&url).await {
                Ok(data) => {
                    tracing::debug!("FetchArtistArt completed for artist {}: {} bytes", artist_id, data.len());
                    ApiResponse::ArtistArt { artist_id, image_data: data }
                },
                Err(e) => ApiResponse::Error(format!("artist art: {e}")),
            }
        }

        ApiRequest::FetchPlaylistArt { uuid, cover_url } => {
            match client.fetch_bytes(&cover_url).await {
                Ok(data) => ApiResponse::PlaylistArt { uuid, image_data: data },
                Err(e) => ApiResponse::Error(format!("playlist art: {e}")),
            }
        }

        ApiRequest::LoadPlaylistTracks { uuid, next_url } => {
            if let Some(next) = &next_url {
                // Subsequent pages use the relationship endpoint
                match client.get_playlist_relationship_items(next, 0).await {
                    Ok((tracks, total, next_url)) => ApiResponse::PlaylistTracks {
                        uuid,
                        tracks,
                        total,
                        next_cursor: next_url,
                        description: None,
                        cover: None,
                    },
                    Err(e) => ApiResponse::Error(e.to_string()),
                }
            } else {
                // First page uses the playlist endpoint
                match client.get_playlist_tracks(&uuid, None).await {
                    Ok((tracks, total, next_url, description, cover)) => ApiResponse::PlaylistTracks {
                        uuid,
                        tracks,
                        total,
                        next_cursor: next_url,
                        description,
                        cover,
                    },
                    Err(e) => ApiResponse::Error(e.to_string()),
                }
            }
        }

        ApiRequest::LoadMixTracks { uuid, offset } => {
            match client.get_mix_tracks(&uuid, offset).await {
                Ok((tracks, total, cover, description)) => ApiResponse::PlaylistTracks {
                    uuid,
                    tracks,
                    total,
                    next_cursor: None,
                    description,
                    cover,
                },
                Err(e) => ApiResponse::Error(e.to_string()),
            }
        }

        ApiRequest::SearchTracks { query } => match client.search_tracks(&query).await {
            Ok(page) => ApiResponse::SearchTracks(page),
            Err(e) => ApiResponse::Error(format!("search tracks: {e}")),
        },

        ApiRequest::SearchArtistsMain { query } => match client.search_artists(&query).await {
            Ok(page) => ApiResponse::SearchArtistsResults(page),
            Err(e) => ApiResponse::Error(format!("search artists: {e}")),
        },

        ApiRequest::SearchPlaylistsMain { query } => match client.search_playlists(&query).await {
            Ok(page) => ApiResponse::SearchPlaylistsResults(page),
            Err(e) => ApiResponse::Error(format!("search playlists: {e}")),
        },

        ApiRequest::SearchTracksNext { next_url } => match client.search_tracks_next(&next_url).await {
            Ok(page) => ApiResponse::SearchTracks(page),
            Err(e) => ApiResponse::Error(format!("search tracks pagination: {e}")),
        },

        ApiRequest::SearchArtistsNext { next_url } => match client.search_artists_next(&next_url).await {
            Ok(page) => ApiResponse::SearchArtistsResults(page),
            Err(e) => ApiResponse::Error(format!("search artists pagination: {e}")),
        },

        ApiRequest::SearchPlaylistsNext { next_url } => match client.search_playlists_next(&next_url).await {
            Ok(page) => ApiResponse::SearchPlaylistsResults(page),
            Err(e) => ApiResponse::Error(format!("search playlists pagination: {e}")),
        },

        ApiRequest::SearchArtistByName { query } => match client.search_artists(&query).await {
            Ok(page) => ApiResponse::SearchedArtists(page.artists),
            Err(e) => ApiResponse::Error(format!("search artists: {e}")),
        },

        ApiRequest::ResolveStreamUrl { track_id } => {
            match client.get_stream_url(track_id).await {
                Ok(url) => ApiResponse::StreamUrl { track_id, url },
                Err(e) => ApiResponse::Error(e.to_string()),
            }
        }

        ApiRequest::FavoriteTrack { track_id } => match client.add_favorite_track(track_id).await {
            Ok(()) => ApiResponse::FavoriteAdded,
            Err(e) => ApiResponse::Error(format!("favorite: {e}")),
        },

        ApiRequest::FollowArtist { artist_id } => match client.follow_artist(artist_id).await {
            Ok(()) => ApiResponse::ArtistFollowed,
            Err(e) => ApiResponse::Error(format!("follow: {e}")),
        },

        ApiRequest::UnfavoriteTrack { track_id } => match client.remove_favorite_track(track_id).await {
            Ok(()) => ApiResponse::FavoriteRemoved { track_id },
            Err(e) => ApiResponse::Error(format!("unfavorite: {e}")),
        },

        ApiRequest::UnfollowArtist { artist_id } => match client.unfollow_artist(artist_id).await {
            Ok(()) => ApiResponse::ArtistUnfollowed { artist_id },
            Err(e) => ApiResponse::Error(format!("unfollow: {e}")),
        },

        ApiRequest::FavoriteAlbum { album_id } => match client.add_favorite_album(album_id).await {
            Ok(()) => ApiResponse::AlbumFavorited { album_id },
            Err(e) => ApiResponse::Error(format!("favorite album: {e}")),
        },

        ApiRequest::UnfavoriteAlbum { album_id } => match client.remove_favorite_album(album_id).await {
            Ok(()) => ApiResponse::AlbumUnfavorited { album_id },
            Err(e) => ApiResponse::Error(format!("unfavorite album: {e}")),
        },

        ApiRequest::SavePlaylist { uuid } => match client.save_playlist(&uuid).await {
            Ok(()) => ApiResponse::PlaylistSaved,
            Err(e) => ApiResponse::Error(format!("save playlist: {e}")),
        },

        ApiRequest::RemovePlaylist { uuid } => match client.remove_playlist(&uuid).await {
            Ok(()) => ApiResponse::PlaylistRemoved { uuid },
            Err(e) => ApiResponse::Error(format!("remove playlist: {e}")),
        },

        ApiRequest::TrackRadio { track_id } => match client.get_track_radio(track_id).await {
            Ok(page) => ApiResponse::RadioTracks { tracks: page.items },
            Err(e) => ApiResponse::Error(format!("radio: {e}")),
        },

        ApiRequest::ArtistRadio { artist_id } => match client.get_artist_radio(artist_id).await {
            Ok(page) => ApiResponse::RadioTracks { tracks: page.items },
            Err(e) => ApiResponse::Error(format!("radio: {e}")),
        },

        ApiRequest::LoadDailyMixes => match client.get_daily_mixes().await {
            Ok(playlists) => ApiResponse::DailyMixes(playlists),
            Err(e) => ApiResponse::Error(format!("daily mixes: {e}")),
        },

        ApiRequest::LoadDiscoveryMixes => match client.get_discovery_mixes().await {
            Ok(playlists) => ApiResponse::DiscoveryMixes(playlists),
            Err(e) => ApiResponse::Error(format!("discovery mixes: {e}")),
        },

        ApiRequest::LoadNewReleases => match client.get_new_release_mixes().await {
            Ok(playlists) => ApiResponse::NewReleases(playlists),
            Err(e) => ApiResponse::Error(format!("new releases: {e}")),
        },

        ApiRequest::GetTrackDetails { track_id } => {
            match client.get_track_details(track_id).await {
                Ok((track, cover_url)) => ApiResponse::TrackDetails { track_id, track, cover_url },
                Err(e) => ApiResponse::Error(format!("track details: {e}")),
            }
        }

        ApiRequest::FetchTrackArt { track_id, cover_url } => {
            match client.fetch_bytes(&cover_url).await {
                Ok(data) => ApiResponse::TrackArt { track_id, image_data: data },
                Err(error) => ApiResponse::TrackArtFailed {
                    track_id,
                    error: error.to_string(),
                },
            }
        }

        ApiRequest::FetchLyrics { track_id } => {
            // A 404 (no lyrics) or any other error → return empty; never emit Error.
            let (synced, plain) = match client.get_track_lyrics(track_id).await {
                Ok(resp) => {
                    let synced = resp.subtitles.as_deref()
                        .filter(|s| !s.is_empty())
                        .map(parse_lrc)
                        .unwrap_or_default();
                    let plain = if synced.is_empty() {
                        resp.lyrics.as_deref().unwrap_or("").lines()
                            .map(str::to_string)
                            .filter(|l| !l.is_empty())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    (synced, plain)
                }
                Err(_) => (Vec::new(), Vec::new()),
            };
            ApiResponse::Lyrics { track_id, synced, plain }
        }
    }
}

const TIDAL_IMAGE_CDN_PREFIX: &str = "https://resources.tidal.com/images/";

fn thumbnail_art_url(cover: &str) -> String {
    if cover.starts_with("http") {
        cover.to_string()
    } else {
        tidal_art_url(cover, "320x320.jpg")
    }
}

fn presentation_art_url(cover: &str) -> String {
    const SIZE: &str = "640x640.jpg";

    // The API can return either a bare image id or an already-expanded Tidal
    // CDN URL. Upgrade only Tidal URLs; custom artwork URLs stay untouched.
    if let Some(path) = cover.strip_prefix(TIDAL_IMAGE_CDN_PREFIX)
        && let Some((image_id, _)) = path.rsplit_once('/')
    {
        return format!("{TIDAL_IMAGE_CDN_PREFIX}{image_id}/{SIZE}");
    }
    if cover.starts_with("http") {
        return cover.to_string();
    }
    tidal_art_url(cover, SIZE)
}

fn tidal_art_url(image_id: &str, size: &str) -> String {
    format!(
        "{TIDAL_IMAGE_CDN_PREFIX}{}/{size}",
        image_id.replace('-', "/")
    )
}

fn parse_lrc(s: &str) -> Vec<(f64, String)> {
    let mut lines = Vec::new();
    for raw in s.lines() {
        let raw = raw.trim();
        if !raw.starts_with('[') {
            continue;
        }
        let Some(close) = raw.find(']') else { continue };
        let tag = &raw[1..close];
        let text = raw[close + 1..].trim().to_string();
        if text.is_empty() {
            continue;
        }
        if let Some(secs) = parse_lrc_time(tag) {
            lines.push((secs, text));
        }
    }
    lines.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

fn parse_lrc_time(s: &str) -> Option<f64> {
    let colon = s.find(':')?;
    let mins: f64 = s[..colon].parse().ok()?;
    let secs: f64 = s[colon + 1..].parse().ok()?;
    Some(mins * 60.0 + secs)
}

/// Strip Tidal's [wimpLink ...]...[/wimpLink] markup, keeping the inner text.
fn strip_wimplinks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        if let Some(close) = rest.find(']') {
            rest = &rest[close + 1..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_art_uses_tidal_medium_resolution_variant() {
        assert_eq!(
            presentation_art_url("33fd4c9b-5673-4c1e-bbd4-5346d397b8e0"),
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/640x640.jpg"
        );
        assert_eq!(
            presentation_art_url(
                "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/320x320.jpg"
            ),
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/640x640.jpg"
        );
    }

    #[test]
    fn normal_album_art_remains_thumbnail_sized() {
        assert_eq!(
            thumbnail_art_url("33fd4c9b-5673-4c1e-bbd4-5346d397b8e0"),
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/320x320.jpg"
        );
        let existing =
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/320x320.jpg";
        assert_eq!(thumbnail_art_url(existing), existing);
    }

    #[test]
    fn presentation_art_preserves_non_tidal_urls() {
        assert_eq!(
            presentation_art_url("https://example.com/custom-cover.png"),
            "https://example.com/custom-cover.png"
        );
    }

    // ── parse_lrc ─────────────────────────────────────────────────────────────

    #[test]
    fn parse_lrc_basic_timestamps() {
        let lines = parse_lrc("[00:01.00]Hello\n[00:02.50]World\n");
        assert_eq!(lines.len(), 2);
        assert!((lines[0].0 - 1.0).abs() < 0.01);
        assert_eq!(lines[0].1, "Hello");
        assert!((lines[1].0 - 2.5).abs() < 0.01);
        assert_eq!(lines[1].1, "World");
    }

    #[test]
    fn parse_lrc_sorts_out_of_order_lines() {
        let lines = parse_lrc("[00:03.00]Third\n[00:01.00]First\n[00:02.00]Second\n");
        assert_eq!(lines[0].1, "First");
        assert_eq!(lines[1].1, "Second");
        assert_eq!(lines[2].1, "Third");
    }

    #[test]
    fn parse_lrc_skips_empty_text() {
        let lines = parse_lrc("[00:01.00]\n[00:02.00]Real\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, "Real");
    }

    #[test]
    fn parse_lrc_ignores_metadata_tags() {
        // Tags like [ti:Title] have no parseable timestamp and should be dropped.
        let lines = parse_lrc("[ti:Title]\n[ar:Artist]\n[00:01.00]Lyric\n");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn parse_lrc_empty_input_returns_empty() {
        assert!(parse_lrc("").is_empty());
    }

    #[test]
    fn parse_lrc_minutes_convert_correctly() {
        let lines = parse_lrc("[02:30.00]Line\n");
        assert!((lines[0].0 - 150.0).abs() < 0.01);
    }

    // ── strip_wimplinks ───────────────────────────────────────────────────────

    #[test]
    fn strip_wimplinks_removes_bracket_tags() {
        let result = strip_wimplinks("[wimpLink href=\"tidal://\"]Artist[/wimpLink]");
        assert_eq!(result, "Artist");
    }

    #[test]
    fn strip_wimplinks_no_tags_unchanged() {
        let s = "Plain biography text with no markup.";
        assert_eq!(strip_wimplinks(s), s);
    }

    #[test]
    fn strip_wimplinks_multiple_tags() {
        let result = strip_wimplinks("[a]Foo[/a] and [b]Bar[/b]");
        assert_eq!(result, "Foo and Bar");
    }

    #[test]
    fn strip_wimplinks_unclosed_tag_preserved() {
        // No closing ']' → the loop breaks and the remaining text (including '[')
        // is appended verbatim. Must not panic.
        let result = strip_wimplinks("Before [unclosed");
        assert_eq!(result, "Before [unclosed");
    }

    #[test]
    fn strip_wimplinks_empty_string() {
        assert_eq!(strip_wimplinks(""), "");
    }
}
