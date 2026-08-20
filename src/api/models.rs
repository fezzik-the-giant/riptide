// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

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

/// Badge for a `mediaTags` set.
///
/// Precedence is by how much the tag tells you: Hi-Res is the rarest claim,
/// spatial audio the next most distinguishing, then plain lossless. `DOLBY_ATMOS`
/// frequently arrives *without* `LOSSLESS` alongside it, so before it had its own
/// branch an Atmos release rendered no badge at all.
fn quality_badge_for(tags: &[String]) -> Option<&'static str> {
    let has = |tag: &str| tags.iter().any(|t| t == tag);
    if has("HIRES_LOSSLESS") {
        Some("MAX")
    } else if has("DOLBY_ATMOS") {
        Some("ATMOS")
    } else if has("LOSSLESS") {
        Some("HI-FI")
    } else {
        None
    }
}

const TIDAL_IMAGE_CDN_PREFIX: &str = "https://resources.tidal.com/images/";

/// Direct CDN URL for a 320×320 cover image. Cover fields carry either a full
/// URL (v2 endpoints) or a dashed resource id that maps onto the CDN path.
pub fn cover_art_url(cover: &str) -> String {
    if cover.starts_with("http") {
        cover.to_owned()
    } else {
        tidal_art_url(cover, "320x320.jpg")
    }
}

/// Direct CDN URL for a 640×640 cover image.
///
/// The API can return either a bare image id or an already-expanded Tidal CDN
/// URL. Upgrade only Tidal URLs; custom artwork URLs stay untouched.
pub fn presentation_art_url(cover: &str) -> String {
    const SIZE: &str = "640x640.jpg";

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
    #[serde(rename = "mediaMetadata", default)]
    pub media_metadata: Option<MediaMetadata>,
    #[serde(default, skip_deserializing)]
    pub added_at: Option<String>,
    #[serde(default, skip_deserializing)]
    pub album_type: Option<String>,
}

impl Album {
    pub fn quality_badge(&self) -> Option<&'static str> {
        quality_badge_for(
            self.media_metadata
                .as_ref()
                .map(|m| m.tags.as_slice())
                .unwrap_or(&[]),
        )
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
        quality_badge_for(
            self.media_metadata
                .as_ref()
                .map(|m| m.tags.as_slice())
                .unwrap_or(&[]),
        )
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

    /// `"23 tracks"`, or `None` when Tidal did not say.
    ///
    /// `MIX` playlists carry no `numberOfItems` at all — verified live, it is
    /// absent from the attributes rather than zero — so rendering the `Option`
    /// as 0 claimed those playlists were empty.
    pub fn track_count_label(&self) -> Option<String> {
        self.number_of_tracks.map(|n| format!("{n} tracks"))
    }
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

/// What Tidal actually delivered for a stream, as opposed to what the catalogue
/// advertises. A `MAX` badge means the release exists in hi-res; it does not mean
/// this client is entitled to be served it, so these are the numbers to show.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeliveredQuality {
    pub bit_depth: Option<i32>,
    pub sample_rate: Option<i32>,
}

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
    #[serde(rename = "bitDepth", default)]
    pub bit_depth: Option<i32>,
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{cover_art_url, presentation_art_url, quality_badge_for};

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn hi_res_outranks_everything() {
        assert_eq!(
            quality_badge_for(&tags(&["HIRES_LOSSLESS", "LOSSLESS"])),
            Some("MAX")
        );
        assert_eq!(
            quality_badge_for(&tags(&["HIRES_LOSSLESS", "DOLBY_ATMOS"])),
            Some("MAX")
        );
    }

    #[test]
    fn atmos_outranks_plain_lossless() {
        // Tidal ships both shapes; verified live against the search endpoint.
        assert_eq!(
            quality_badge_for(&tags(&["DOLBY_ATMOS", "LOSSLESS"])),
            Some("ATMOS")
        );
    }

    #[test]
    fn atmos_alone_is_badged() {
        // This set rendered no badge at all before ATMOS had its own branch —
        // Atmos releases often carry no LOSSLESS tag beside it.
        assert_eq!(quality_badge_for(&tags(&["DOLBY_ATMOS"])), Some("ATMOS"));
    }

    #[test]
    fn lossless_alone_is_hi_fi() {
        assert_eq!(quality_badge_for(&tags(&["LOSSLESS"])), Some("HI-FI"));
    }

    #[test]
    fn no_tags_means_no_badge() {
        assert_eq!(quality_badge_for(&[]), None);
        assert_eq!(quality_badge_for(&tags(&["SOMETHING_NEW"])), None);
    }

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
    fn thumbnail_art_remains_the_source_size() {
        assert_eq!(
            cover_art_url("33fd4c9b-5673-4c1e-bbd4-5346d397b8e0"),
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/320x320.jpg"
        );
        let existing =
            "https://resources.tidal.com/images/33fd4c9b/5673/4c1e/bbd4/5346d397b8e0/320x320.jpg";
        assert_eq!(cover_art_url(existing), existing);
    }

    #[test]
    fn presentation_art_preserves_non_tidal_urls() {
        assert_eq!(
            presentation_art_url("https://example.com/custom-cover.png"),
            "https://example.com/custom-cover.png"
        );
    }
}
