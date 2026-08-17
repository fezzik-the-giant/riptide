// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use serde::{Deserialize, Serialize};

// ── Pagination envelope ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Page<T> {
    #[serde(rename = "totalNumberOfItems")]
    #[allow(dead_code)]
    pub total: u32,
    pub items: Vec<T>,
}

// ── References ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct ArtistRef {
    pub name: String,
}

// ── Artists ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct Artist {
    pub id: u64,
    pub name: String,
    #[serde(default, skip_deserializing)]
    pub added_at: Option<String>,
}

impl Artist {
    /// Public Tidal share URL, matching the "Copy link" output of the official apps.
    pub fn share_url(&self) -> String {
        format!("https://tidal.com/browse/artist/{}", self.id)
    }
}

// ── Albums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone, Default)]
pub struct MediaMetadata {
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Album {
    pub id: u64,
    pub title: String,
    #[serde(rename = "numberOfTracks")]
    pub number_of_tracks: Option<u32>,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
    pub cover: Option<String>,
    pub artist: Option<ArtistRef>,
    #[serde(rename = "audioQuality", default)]
    pub audio_quality: Option<String>,
    #[serde(rename = "mediaMetadata", default)]
    pub media_metadata: Option<MediaMetadata>,
    #[serde(default, skip_deserializing)]
    pub added_at: Option<String>,
    #[serde(default, skip_deserializing)]
    pub album_type: Option<String>,
}

impl Album {
    pub fn quality_badge(&self) -> Option<&'static str> {
        let tags = self
            .media_metadata
            .as_ref()
            .map(|m| m.tags.as_slice())
            .unwrap_or(&[]);
        if tags.iter().any(|t| t == "HIRES_LOSSLESS") {
            return Some("MAX");
        }
        if tags.iter().any(|t| t == "LOSSLESS") {
            return Some("HI-FI");
        }
        match self.audio_quality.as_deref() {
            Some("HI_RES") => Some("MQA"),
            Some("HIGH") => Some("320"),
            _ => None,
        }
    }
    pub fn artist_name(&self) -> &str {
        self.artist.as_ref().map(|a| a.name.as_str()).unwrap_or("")
    }

    /// Public Tidal share URL, matching the "Copy link" output of the official apps.
    pub fn share_url(&self) -> String {
        format!("https://tidal.com/browse/album/{}", self.id)
    }
}

// ── Tracks ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub duration: u32,
    /// Present on most endpoints; absent on search results which use `artists`.
    pub artist: Option<ArtistRef>,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    pub album: Album,
    #[serde(rename = "audioQuality")]
    pub audio_quality: Option<String>,
    #[serde(rename = "mediaMetadata", default)]
    pub media_metadata: Option<MediaMetadata>,
    #[serde(default, skip_deserializing)]
    pub added_at: Option<String>,
}

impl Track {
    pub fn duration_display(&self) -> String {
        let m = self.duration / 60;
        let s = self.duration % 60;
        format!("{m}:{s:02}")
    }

    pub fn artist_name(&self) -> &str {
        self.artist
            .as_ref()
            .or_else(|| self.artists.first())
            .map(|a| a.name.as_str())
            .unwrap_or("")
    }

    pub fn all_artist_names(&self) -> String {
        if !self.artists.is_empty() {
            self.artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            self.artist_name().to_string()
        }
    }

    pub fn quality_badge(&self) -> Option<&'static str> {
        let tags = self
            .media_metadata
            .as_ref()
            .map(|m| m.tags.as_slice())
            .unwrap_or(&[]);
        if tags.iter().any(|t| t == "HIRES_LOSSLESS") {
            return Some("MAX");
        }
        if tags.iter().any(|t| t == "LOSSLESS") {
            return Some("HI-FI");
        }
        match self.audio_quality.as_deref() {
            Some("HI_RES") => Some("MQA"),
            Some("HIGH") => Some("320"),
            _ => None,
        }
    }

    pub fn quality_display(&self) -> &str {
        match self.audio_quality.as_deref() {
            Some("HI_RES_LOSSLESS") => "Hi-Res",
            Some("HI_RES") => "MQA",
            Some("LOSSLESS") => "FLAC",
            Some("HIGH") => "AAC 320",
            Some("LOW") => "AAC 96",
            Some(other) => other,
            None => "",
        }
    }

    /// Public Tidal share URL, matching the "Copy link" output of the official apps.
    pub fn share_url(&self) -> String {
        format!("https://tidal.com/browse/track/{}", self.id)
    }
}

// ── Playlists ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    #[serde(rename = "numberOfTracks", default)]
    pub number_of_tracks: Option<u32>,
    #[serde(default)]
    pub description: Option<String>,
    pub cover: Option<String>,
    #[serde(default, skip_deserializing)]
    pub added_at: Option<String>,
}

impl Playlist {
    /// Public Tidal share URL, matching the "Copy link" output of the official apps.
    pub fn share_url(&self) -> String {
        format!("https://tidal.com/browse/playlist/{}", self.uuid)
    }
}

#[derive(Debug, Deserialize)]
pub struct FavoritePlaylistEntry {
    pub created: Option<String>,
    // Tidal uses "playlist" here; every other favorites endpoint uses "item".
    #[serde(alias = "item")]
    pub playlist: Playlist,
}

// ── Search ────────────────────────────────────────────────────────────────────

// ── Lyrics ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LyricsResponse {
    pub lyrics: Option<String>,
    /// LRC-format timed subtitles, when available.
    pub subtitles: Option<String>,
}

// ── Stream URL ────────────────────────────────────────────────────────────────

/// Response from /tracks/{id}/playbackinfopostpaywall
#[derive(Debug, Deserialize)]
pub struct PlaybackInfo {
    #[serde(rename = "manifestMimeType")]
    pub manifest_mime_type: String,
    pub manifest: String,
    #[allow(dead_code)]
    #[serde(rename = "audioQuality", default)]
    pub audio_quality: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "audioMode", default)]
    pub audio_mode: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "bitDepth", default)]
    pub bit_depth: Option<i32>,
    #[allow(dead_code)]
    #[serde(rename = "sampleRate", default)]
    pub sample_rate: Option<i32>,
}

/// Decoded content of a `application/vnd.tidal.bts` manifest.
///
/// For LOSSLESS quality the `mimeType` is `"audio/flac"` and `codecs` is `"flac"`.
/// For HIGH / LOW the codecs is something like `"mp4a.40.2"` (AAC).
#[derive(Debug, Deserialize)]
pub struct BtsManifest {
    #[allow(dead_code)]
    #[serde(rename = "mimeType", default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub codecs: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "encryptionType", default)]
    pub encryption_type: Option<String>,
    pub urls: Vec<String>,
}

impl BtsManifest {
    /// True when the manifest's codec is FLAC (i.e. real lossless).
    pub fn is_flac(&self) -> bool {
        self.codecs.as_deref() == Some("flac")
    }

    /// True when the manifest codec is an AAC variant.
    #[allow(dead_code)]
    pub fn is_aac(&self) -> bool {
        self.codecs
            .as_deref()
            .map(|c| c.starts_with("mp4a"))
            .unwrap_or(false)
    }
}

// ── Sessions ──────────────────────────────────────────────────────────────────

/// Response from GET /sessions — needed after every fresh auth.
#[derive(Debug, Deserialize)]
pub struct SessionInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "userId")]
    pub user_id: u64,
    #[serde(rename = "countryCode")]
    pub country_code: String,
}

// ── OAuth ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeviceAuthResponse {
    #[serde(rename = "deviceCode")]
    pub device_code: String,
    #[serde(rename = "userCode")]
    pub user_code: String,
    #[serde(rename = "verificationUriComplete")]
    pub verification_uri_complete: String,
    pub interval: u32,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub user: Option<TokenUser>,
}

#[derive(Debug, Deserialize)]
pub struct TokenUser {
    #[serde(rename = "userId")]
    pub user_id: u64,
    #[serde(rename = "countryCode")]
    pub country_code: String,
}

// ── Config (persisted to disk) ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// Override the OAuth client ID. Falls back to the built-in default when absent.
    pub client_id: Option<String>,
    /// Override the OAuth client secret. Falls back to the built-in default when absent.
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// RFC 3339 expiry timestamp
    pub expires_at: Option<String>,
    pub user_id: Option<u64>,
    pub country_code: String,
    /// Tidal session UUID — required as `sessionId` query param on all v1 requests.
    pub session_id: Option<String>,
    /// Last.fm scrobbling configuration
    #[serde(default)]
    pub lastfm: crate::lastfm::LastfmConfig,
    /// UI choices that persist across restarts (sort orders, volume, shuffle…).
    #[serde(default)]
    pub prefs: crate::app::Preferences,
    /// Tracks which client credentials / auth method was used.
    /// 0 = pre-migration (AAC-only, form-field auth, old client ID).
    /// 1 = tiddl credentials + HTTP Basic Auth (lossless-capable).
    /// Bumped to force re-auth when credentials or auth method change.
    #[serde(default)]
    pub auth_generation: u32,
}
