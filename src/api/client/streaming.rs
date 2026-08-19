// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Stream URL resolution.
//!
//! This is the one place still on the Tidal v1 API, deliberately: the v2
//! trackManifests endpoint only serves CENC-encrypted media. See the doc
//! comment on get_stream_url for the full reasoning.

use anyhow::{Context, Result};
use base64::Engine as _;

use super::{ApiClient, BASE};
use crate::api::models::*;

/// Build a simple M3U8 playlist for multi-segment raw FLAC URLs so mpv can
/// play them gaplessly in sequence.
fn build_flac_m3u8(track_id: u64, urls: &[String]) -> String {
    let mut m3u8 = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    // Each segment is a standalone FLAC file; mpv handles concatenation natively.
    for url in urls {
        // We don't know exact durations upfront, but mpv will determine them
        // from the FLAC stream headers. Use a generous placeholder.
        m3u8.push_str("#EXTINF:10.0,\n");
        m3u8.push_str(url);
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    let playlist_path = format!("/tmp/riptide_hls_{track_id}.m3u8");
    let _ = std::fs::write(&playlist_path, &m3u8);
    format!("http://127.0.0.1:{}/{track_id}.m3u8", crate::manifest::PORT)
}

/// Represents a single `<AdaptationSet>` found in the DASH manifest.
struct DashAdaptationSet {
    /// The `codecs` attribute from the AdaptationSet or Representation element.
    codecs: String,
    /// Position of this AdaptationSet in the original XML (byte offset of opening tag).
    _offset: usize,
}

/// Find all AdaptationSet elements and their codec info.
/// Returns them so we can prefer FLAC over AAC.
fn find_adaptation_sets(xml: &str) -> Vec<DashAdaptationSet> {
    let mut sets = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find("<AdaptationSet") {
        let set_start = pos;
        let fragment = &rest[pos..];
        // Find codecs in the AdaptationSet or its Representation child.
        let codecs = dash_attr(fragment, "codecs").unwrap_or_default();
        let offset = xml.len() - rest.len() + set_start;
        sets.push(DashAdaptationSet {
            codecs,
            _offset: offset,
        });
        // Advance past this AdaptationSet to find the next one.
        if let Some(end) = fragment.find("</AdaptationSet>") {
            rest = &fragment[end + "</AdaptationSet>".len()..];
        } else {
            break;
        }
    }
    sets
}

/// Convert a Tidal DASH manifest to an HLS playlist served via local HTTP.
///
/// When the manifest contains multiple AdaptationSets (e.g. AAC and FLAC),
/// we prefer the FLAC one so mpv plays real lossless audio.
fn dash_to_hls(track_id: u64, xml: &str) -> anyhow::Result<String> {
    // If there are multiple AdaptationSets, try to find a FLAC one.
    let adaptation_sets = find_adaptation_sets(xml);

    // Determine which region of the XML to use for attribute extraction.
    // If we have a FLAC adaptation set, extract attributes from within it.
    let search_region = if adaptation_sets.len() > 1 {
        if let Some(flac_set) = adaptation_sets.iter().find(|s| s.codecs == "flac") {
            // Extract from just this AdaptationSet's region of the XML.
            let start = flac_set._offset;
            let rest = &xml[start..];
            if let Some(end) = rest.find("</AdaptationSet>") {
                &rest[..end + "</AdaptationSet>".len()]
            } else {
                xml
            }
        } else {
            xml
        }
    } else {
        xml
    };

    let codecs = dash_attr(search_region, "codecs").unwrap_or_default();

    let init_url = dash_attr(search_region, "initialization")
        .context("no initialization URL in DASH manifest")?;
    let media_tmpl =
        dash_attr(search_region, "media").context("no media template in DASH manifest")?;
    let timescale: f64 = dash_attr(search_region, "timescale")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let start_num: u64 = dash_attr(search_region, "startNumber")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let durations = dash_segment_durations(search_region, timescale);
    anyhow::ensure!(!durations.is_empty(), "no segments in DASH manifest");

    let target = durations.iter().cloned().fold(0f64, f64::max).ceil() as u64;
    let mut m3u8 = format!("#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-TARGETDURATION:{target}\n");
    // Include codec info so mpv knows what to expect.
    if !codecs.is_empty() {
        m3u8.push_str(&format!("#EXT-X-CODECS:{codecs}\n"));
    }
    m3u8.push_str(&format!("#EXT-X-MAP:URI=\"{init_url}\"\n"));

    for (i, dur) in durations.iter().enumerate() {
        m3u8.push_str(&format!("#EXTINF:{dur:.5},\n"));
        m3u8.push_str(&media_tmpl.replace("$Number$", &(start_num + i as u64).to_string()));
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    std::fs::write(format!("/tmp/riptide_hls_{track_id}.m3u8"), &m3u8)
        .context("write HLS playlist")?;
    Ok(format!(
        "http://127.0.0.1:{}/{track_id}.m3u8",
        crate::manifest::PORT
    ))
}

/// Extract an XML attribute value by name, checking that it isn't a substring
/// of a longer attribute name (e.g. `d` must not match `id`).
fn dash_attr(xml: &str, name: &str) -> Option<String> {
    let needle = format!("{}=\"", name);
    let mut haystack = xml;
    while let Some(pos) = haystack.find(&needle) {
        let before = pos
            .checked_sub(1)
            .and_then(|i| haystack.as_bytes().get(i).copied())
            .map(|b| b as char)
            .unwrap_or(' ');
        if !before.is_alphanumeric() && before != '_' && before != '-' {
            let start = pos + needle.len();
            let end = haystack[start..].find('"')? + start;
            return Some(haystack[start..end].to_owned());
        }
        haystack = &haystack[pos + needle.len()..];
    }
    None
}

/// Parse `<S d="..." r="..."/>` elements inside `<SegmentTimeline>`.
fn dash_segment_durations(xml: &str, timescale: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let tl_start = match xml.find("<SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let tl = &xml[tl_start..];
    let tl_end = match tl.find("</SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let mut rest = &tl[..tl_end];
    while let Some(pos) = rest.find("<S ") {
        let inner_start = pos + 3;
        let inner_end = rest[inner_start..]
            .find("/>")
            .map(|p| p + inner_start)
            .unwrap_or(rest.len());
        let elem = &rest[inner_start..inner_end];
        let d: f64 = dash_attr(elem, "d")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let r: usize = dash_attr(elem, "r")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let dur = d / timescale;
        for _ in 0..=r {
            out.push(dur);
        }
        rest = &rest[inner_end..];
    }
    out
}

impl ApiClient {
    /// Resolve a playable URL for `track_id`.
    ///
    /// # This stays on v1 permanently — do not migrate it
    ///
    /// Every other endpoint in this client has moved to openapi.tidal.com/v2.
    /// This one cannot, and the reason is DRM rather than effort. Verified
    /// against track 431291038 on a live subscriber token:
    ///
    /// v1 `playbackinfopostpaywall` (LOSSLESS) returns a BTS manifest reading
    /// `{"codecs":"flac","encryptionType":"NONE","urls":[...]}` — a plain HTTPS
    /// URL to an unencrypted FLAC that mpv plays directly.
    ///
    /// The v2 equivalent, `GET /trackManifests/{id}`, only accepts
    /// `manifestType` of `HLS` or `MPEG_DASH` (no BTS), and every combination
    /// of `formats` / `usage` / `adaptive` returns CENC-encrypted content:
    ///
    /// | manifestType | formats           | Result                              |
    /// |--------------|-------------------|-------------------------------------|
    /// | HLS          | FLAC, FLAC_HIRES  | FairPlay, `initData: ["skd://…"]`   |
    /// | MPEG_DASH    | FLAC, FLAC_HIRES  | Widevine                            |
    /// | MPEG_DASH    | AACLC             | Widevine                            |
    /// | MPEG_DASH    | `usage=DOWNLOAD`  | Widevine                            |
    ///
    /// The `"drmData": {"initData": null}` on the DASH responses is a red
    /// herring — the init data lives in the `.mpd` body itself:
    /// `<ContentProtection value="cbcs" cenc:default_KID="…">` plus Widevine
    /// (`edef8ba9-…`) and PlayReady (`9a04f079-…`) PSSH boxes.
    ///
    /// Decrypting that needs a CDM, which mpv/ffmpeg do not have. TIDAL's own
    /// Player SDK solves it by delegating to a browser's EME stack
    /// (`shaka-player`, `fairplay-drm.ts`), so adopting the SDK would mean
    /// embedding a browser engine and dropping mpv — and third-party apps on
    /// that path are limited to 30-second previews unless the client ID is
    /// entitled. Ours is not: `/trackFiles/{id}` returns
    /// `403 CLIENT_NOT_ENTITLED`, and `/tracks/{id}/relationships/download`
    /// yields only a resource identifier, no file.
    ///
    /// v1 also already carries the loudness data v2 advertises
    /// (`albumReplayGain`, `trackReplayGain`, and both peak amplitudes), so
    /// there is nothing to gain by switching even setting DRM aside.
    ///
    /// Consequence: `BASE`, `dash_to_hls`, `build_flac_m3u8` and the localhost
    /// manifest server in `src/manifest.rs` are all load-bearing and must stay.
    pub async fn get_stream_url(&self, track_id: u64) -> Result<(String, DeliveredQuality)> {
        // Quality fallback chain for streaming.
        //
        // | Quality          | Manifest MIME type         | Container   | Actual codec  |
        // |------------------|----------------------------|-------------|---------------|
        // | LOSSLESS         | application/vnd.tidal.bts  | audio/flac  | FLAC (raw)    |
        // | HI_RES_LOSSLESS  | application/dash+xml       | audio/mp4   | FLAC or AAC   |
        // | HIGH             | application/vnd.tidal.bts  | audio/mp4   | AAC           |
        //
        // LOSSLESS → BTS manifest with `codecs: "flac"` → guaranteed raw FLAC.
        // HI_RES_LOSSLESS → DASH manifest where codecs MAY be "flac" or "mp4a.40.2".
        // Strategy: try LOSSLESS first (guaranteed FLAC), then HI_RES_LOSSLESS
        // (only if its DASH codec is actually FLAC), then HIGH as last resort.
        const QUALITIES: &[&str] = &["LOSSLESS", "HI_RES_LOSSLESS", "HIGH"];
        let path = format!("/tracks/{track_id}/playbackinfopostpaywall");
        let debug = std::env::var("RIPTIDE_QUALITY_DEBUG").is_ok();
        let token = self.token.read().await.clone();
        let base_url = format!("{BASE}{path}");

        for &quality in QUALITIES {
            let mut all_params: Vec<(&str, String)> = vec![
                ("countryCode", self.config.country_code.clone()),
                ("audioquality", quality.to_string()),
                ("playbackmode", "STREAM".to_string()),
                ("assetpresentation", "FULL".to_string()),
            ];
            if let Some(sid) = &self.config.session_id {
                all_params.push(("sessionId", sid.clone()));
            }

            let resp = self
                .http
                .get(&base_url)
                .bearer_auth(&token.clone())
                .query(&all_params)
                .send()
                .await
                .context("HTTP request failed")?;

            let status = resp.status();

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                let error_msg = if body.is_empty() {
                    status.to_string()
                } else {
                    // Truncate to first 200 chars for readability
                    let snippet: String = body.chars().take(200).collect();
                    format!("{}: {}", status, snippet)
                };

                if debug {
                    eprintln!("[quality] track {track_id}: {quality} request failed — {error_msg}");
                }

                if matches!(
                    status,
                    reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
                ) {
                    tracing::debug!("Track {track_id} ({quality}): {error_msg}");
                    continue;
                }
                return Err(anyhow::anyhow!("Track {track_id} ({quality}): {error_msg}"));
            }

            let body = resp.text().await?;
            let info: PlaybackInfo =
                serde_json::from_str(&body).context("parse playback info response")?;

            let delivered = DeliveredQuality {
                bit_depth: info.bit_depth,
                sample_rate: info.sample_rate,
            };
            let mime = info.manifest_mime_type.clone();
            if debug {
                let aq = info.audio_quality.as_deref().unwrap_or("?");
                eprintln!(
                    "[quality] track {track_id}: requested {quality}, \
                     server returned manifestMimeType={mime}, \
                     audioQuality={aq} (200 OK)",
                );
            }

            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&info.manifest)
                .context("base64 decode of manifest")?;

            match mime.as_str() {
                "application/vnd.tidal.bts" => {
                    let manifest: BtsManifest =
                        serde_json::from_slice(&bytes).context("parse BTS manifest")?;

                    if manifest.urls.is_empty() {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: BTS manifest has empty urls — skip"
                            );
                        }
                        continue;
                    }

                    let codec = manifest.codecs.as_deref().unwrap_or("(missing)");
                    if debug {
                        eprintln!(
                            "[quality] track {track_id}: BTS codecs={codec}, \
                             urls={} segment(s)",
                            manifest.urls.len(),
                        );
                    }

                    // BTS with FLAC codec → real lossless.
                    if manifest.is_flac() {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: ✓ FLAC stream accepted ({quality})"
                            );
                        }
                        if manifest.urls.len() == 1 {
                            return Ok((manifest.urls.into_iter().next().unwrap(), delivered));
                        }
                        let m3u8 = build_flac_m3u8(track_id, &manifest.urls);
                        return Ok((m3u8, delivered));
                    }

                    // BTS with non-FLAC codec.
                    // For LOSSLESS requests: the API downgraded us → skip.
                    // For HIGH requests: this is expected AAC → accept.
                    if quality == "HIGH" {
                        if debug {
                            eprintln!("[quality] track {track_id}: accepting AAC stream (HIGH)");
                        }
                        if let Some(url) = manifest.urls.into_iter().next() {
                            return Ok((url, delivered));
                        }
                    } else {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: BTS codec is '{codec}' \
                                 (not flac) for {quality} request — falling through",
                            );
                        }
                        continue;
                    }
                }
                "application/dash+xml" => {
                    let xml = String::from_utf8_lossy(&bytes);

                    let sets = find_adaptation_sets(&xml);
                    let has_flac = sets.iter().any(|s| s.codecs == "flac");

                    if debug {
                        let codecs: Vec<&str> = sets.iter().map(|s| s.codecs.as_str()).collect();
                        eprintln!(
                            "[quality] track {track_id}: DASH with {} AdaptationSet(s), \
                             codecs={:?}, has_flac={has_flac}",
                            sets.len(),
                            codecs,
                        );
                    }

                    if (quality == "LOSSLESS" || quality == "HI_RES_LOSSLESS") && !has_flac {
                        if debug {
                            eprintln!(
                                "[quality] track {track_id}: DASH has no FLAC codec \
                                 — falling through to next tier",
                            );
                        }
                        continue;
                    }

                    if debug {
                        eprintln!("[quality] track {track_id}: ✓ DASH/FLAC accepted ({quality})");
                    }
                    let hls =
                        dash_to_hls(track_id, &xml).context("convert DASH manifest to HLS")?;
                    return Ok((hls, delivered));
                }
                _ => {
                    if debug {
                        eprintln!(
                            "[quality] track {track_id}: unknown manifest MIME type '{mime}' — skip",
                        );
                    }
                    continue;
                }
            }
        }

        Err(anyhow::anyhow!(
            "no stream URL available for track {track_id}"
        ))
    }
}
