// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

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
    ///
    /// # Dolby Atmos
    ///
    /// `atmos` becomes the `immersiveaudio` query parameter. Verified live
    /// against track 189681882 (`DOLBY_ATMOS`) and 431291038 (stereo):
    ///
    /// | immersiveaudio | Atmos track                        | stereo track |
    /// |----------------|------------------------------------|--------------|
    /// | `true`         | `audioMode=DOLBY_ATMOS`, BTS `eac3` | unchanged    |
    /// | `false`        | `audioMode=STEREO`, BTS `flac`      | unchanged    |
    /// | omitted        | as `true` — Atmos is the default    | unchanged    |
    ///
    /// So the parameter is always sent: absence is not the same as `false`, and
    /// only the literal string `true` enables Atmos — `False`, `0` and junk all
    /// read as off. It is inert on tracks that have no Atmos mix, which is why
    /// it needs no per-track check.
    ///
    /// The Atmos stream is `encryptionType: NONE`, so it plays directly. ffmpeg
    /// decodes the E-AC-3 5.1 core and ignores the JOC object metadata, so the
    /// setting buys real surround on a surround setup and a lossy downmix of a
    /// lossless-capable release on two speakers. That is the trade the setting
    /// exists to let the listener make.
    pub async fn get_stream_url(
        &self,
        track_id: u64,
        atmos: bool,
    ) -> Result<(String, DeliveredQuality)> {
        let hires_path = crate::api::auth::hires_config_path();
        self.get_stream_url_at(BASE, track_id, atmos, Some(&hires_path))
            .await
    }

    async fn get_stream_url_at(
        &self,
        base: &str,
        track_id: u64,
        atmos: bool,
        hires_path: Option<&std::path::Path>,
    ) -> Result<(String, DeliveredQuality)> {
        // Quality fallback chain for streaming.
        //
        // | Quality          | Manifest MIME type         | Container   | Actual codec  |
        // |------------------|----------------------------|-------------|---------------|
        // | LOSSLESS         | application/vnd.tidal.bts  | audio/flac  | FLAC (raw)    |
        // | HI_RES_LOSSLESS  | application/dash+xml       | audio/mp4   | FLAC or AAC   |
        // | HIGH             | application/vnd.tidal.bts  | audio/mp4   | AAC           |
        //
        // LOSSLESS normally returns BTS FLAC through the primary client.
        // HI_RES_LOSSLESS → DASH manifest where codecs MAY be "flac" or "mp4a.40.2".
        // Prefer hi-res FLAC, then ordinary lossless, then AAC. Tidal may
        // downgrade a request or return encrypted/unsupported media; keep
        // looking for playable FLAC before accepting a lossy fallback.
        //
        // An Atmos release with `atmos` on answers every tier with the same BTS
        // `eac3` manifest, so the chain is decided on the first request.
        const QUALITIES: &[&str] = &["HI_RES_LOSSLESS", "LOSSLESS", "HIGH"];
        let path = format!("/tracks/{track_id}/playbackinfopostpaywall");
        let debug = std::env::var("RIPTIDE_QUALITY_DEBUG").is_ok();
        let base_url = format!("{base}{path}");
        let mut last_tier_error = None;
        let hires = if let Some(path) = hires_path.filter(|_| !atmos) {
            match super::hires::load(&self.http, path, self.config.user_id).await {
                Ok(config) => config,
                Err(error) => {
                    tracing::warn!("Hi-res login unavailable; using primary login: {error:#}");
                    None
                }
            }
        } else {
            None
        };

        for &quality in QUALITIES {
            let use_hires = quality == "HI_RES_LOSSLESS" && hires.is_some();
            let config = if use_hires {
                hires.as_ref().unwrap()
            } else {
                &self.config
            };
            let token = if use_hires {
                config.access_token.clone().unwrap_or_default()
            } else {
                self.access_token().await?
            };
            let mut all_params: Vec<(&str, String)> = vec![
                ("countryCode", config.country_code.clone()),
                ("audioquality", quality.to_string()),
                ("playbackmode", "STREAM".to_string()),
                ("assetpresentation", "FULL".to_string()),
                ("immersiveaudio", atmos.to_string()),
            ];
            if let Some(sid) = &config.session_id {
                all_params.push(("sessionId", sid.clone()));
            }

            let resp = self
                .http
                .get(&base_url)
                .bearer_auth(token.clone())
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
                    tracing::debug!(target: "riptide::quality", "[quality] track {track_id}: {quality} request failed — {error_msg}");
                }

                // Tidal also uses HTTP 401 for an unavailable asset (4005),
                // even while the same session can play a lower quality.
                // Other primary-login 401s stop immediately; a rejected
                // hi-res login can still fall back to the primary session.
                let asset_unavailable = status == reqwest::StatusCode::UNAUTHORIZED
                    && serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|error| error.get("subStatus").and_then(|v| v.as_u64()))
                        == Some(4005);
                if status == reqwest::StatusCode::FORBIDDEN
                    || asset_unavailable
                    || (use_hires && status == reqwest::StatusCode::UNAUTHORIZED)
                {
                    tracing::debug!("Track {track_id} ({quality}): {error_msg}");
                    last_tier_error = Some(format!("{quality}: {error_msg}"));
                    continue;
                }
                if status == reqwest::StatusCode::UNAUTHORIZED {
                    anyhow::bail!(
                        "Tidal rejected the session (401). The access token could not be \
                         renewed — restart riptide to sign in again. ({error_msg})"
                    );
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
                tracing::debug!(target: "riptide::quality",
                    "[quality] track {track_id}: requested {quality}, \
                     server returned manifestMimeType={mime}, \
                     audioQuality={aq}, bitDepth={:?}, sampleRate={:?} (200 OK)",
                    info.bit_depth,
                    info.sample_rate,
                );
            }

            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&info.manifest)
                .context("base64 decode of manifest")?;

            match mime.as_str() {
                "application/vnd.tidal.bts" => {
                    let manifest: BtsManifest =
                        serde_json::from_slice(&bytes).context("parse BTS manifest")?;

                    if manifest
                        .encryption_type
                        .as_deref()
                        .is_some_and(|kind| kind != "NONE")
                    {
                        if debug {
                            tracing::debug!(target: "riptide::quality", "[quality] track {track_id}: encrypted BTS — skip");
                        }
                        continue;
                    }

                    if manifest.urls.is_empty() {
                        if debug {
                            tracing::debug!(target: "riptide::quality",
                                "[quality] track {track_id}: BTS manifest has empty urls — skip"
                            );
                        }
                        continue;
                    }

                    let codec = manifest.codecs.as_deref().unwrap_or("(missing)");
                    if debug {
                        tracing::debug!(target: "riptide::quality",
                            "[quality] track {track_id}: BTS codecs={codec}, \
                             urls={} segment(s)",
                            manifest.urls.len(),
                        );
                    }

                    // BTS with FLAC codec → real lossless.
                    if manifest.is_flac() {
                        if debug {
                            tracing::debug!(target: "riptide::quality",
                                "[quality] track {track_id}: ✓ FLAC stream accepted (requested {quality})"
                            );
                        }
                        if manifest.urls.len() == 1 {
                            return Ok((manifest.urls.into_iter().next().unwrap(), delivered));
                        }
                        let m3u8 = build_flac_m3u8(track_id, &manifest.urls);
                        return Ok((m3u8, delivered));
                    }

                    // Atmos, which can only be here because we asked for it.
                    // It fails the FLAC test above, so without this branch every
                    // Atmos track was requested three times over before the HIGH
                    // tier accepted the very stream the first tier had returned.
                    if manifest.is_eac3() {
                        if debug {
                            tracing::debug!(target: "riptide::quality",
                                "[quality] track {track_id}: ✓ Dolby Atmos stream accepted (requested {quality})"
                            );
                        }
                        return Ok((manifest.urls.into_iter().next().unwrap(), delivered));
                    }

                    // BTS with non-FLAC codec.
                    // For LOSSLESS requests: the API downgraded us → skip.
                    // For HIGH requests: this is expected AAC → accept.
                    if quality == "HIGH" {
                        if debug {
                            tracing::debug!(target: "riptide::quality", "[quality] track {track_id}: accepting AAC stream (HIGH)");
                        }
                        if let Some(url) = manifest.urls.into_iter().next() {
                            return Ok((url, delivered));
                        }
                    } else {
                        if debug {
                            tracing::debug!(target: "riptide::quality",
                                "[quality] track {track_id}: BTS codec is '{codec}' \
                                 (not flac) for {quality} request — falling through",
                            );
                        }
                        continue;
                    }
                }
                "application/dash+xml" => {
                    let xml = String::from_utf8_lossy(&bytes);

                    // mpv has no CDM. Check both default and prefixed XML
                    // namespaces before handing an encrypted stream to it.
                    if xml.contains("<ContentProtection") || xml.contains(":ContentProtection") {
                        if debug {
                            tracing::debug!(target: "riptide::quality", "[quality] track {track_id}: encrypted DASH — skip");
                        }
                        continue;
                    }

                    let sets = find_adaptation_sets(&xml);
                    let has_flac = sets.iter().any(|s| s.codecs == "flac");

                    if debug {
                        let codecs: Vec<&str> = sets.iter().map(|s| s.codecs.as_str()).collect();
                        tracing::debug!(target: "riptide::quality",
                            "[quality] track {track_id}: DASH with {} AdaptationSet(s), \
                             codecs={:?}, has_flac={has_flac}",
                            sets.len(),
                            codecs,
                        );
                    }

                    if (quality == "LOSSLESS" || quality == "HI_RES_LOSSLESS") && !has_flac {
                        if debug {
                            tracing::debug!(target: "riptide::quality",
                                "[quality] track {track_id}: DASH has no FLAC codec \
                                 — falling through to next tier",
                            );
                        }
                        continue;
                    }

                    let hls = match dash_to_hls(track_id, &xml) {
                        Ok(hls) => hls,
                        Err(_) => {
                            if debug {
                                tracing::debug!(target: "riptide::quality",
                                    "[quality] track {track_id}: unsupported DASH manifest — skip"
                                );
                            }
                            continue;
                        }
                    };
                    if debug {
                        tracing::debug!(target: "riptide::quality", "[quality] track {track_id}: ✓ DASH/FLAC accepted (requested {quality})");
                    }
                    return Ok((hls, delivered));
                }
                _ => {
                    if debug {
                        tracing::debug!(target: "riptide::quality",
                            "[quality] track {track_id}: unknown manifest MIME type '{mime}' — skip",
                        );
                    }
                    continue;
                }
            }
        }

        if let Some(error) = last_tier_error {
            anyhow::bail!("No playable quality available for track {track_id} ({error})");
        }
        Err(anyhow::anyhow!(
            "no stream URL available for track {track_id}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};

    const DASH: &str = r#"<MPD><Period><AdaptationSet codecs="flac">
            <Representation audioSamplingRate="96000">
                <SegmentTemplate timescale="96000" startNumber="1"
                    initialization="https://example.test/init.mp4"
                    media="https://example.test/$Number$.m4s">
                    <SegmentTimeline><S d="192000" r="1"/></SegmentTimeline>
                </SegmentTemplate>
            </Representation>
        </AdaptationSet></Period></MPD>"#;

    fn playback(mime: &str, manifest: &str, bits: i32, rate: i32) -> Value {
        json!({
            "manifestMimeType": mime,
            "manifest": base64::engine::general_purpose::STANDARD.encode(manifest),
            "bitDepth": bits,
            "sampleRate": rate,
        })
    }

    fn bts(codec: &str, encryption: &str) -> Value {
        playback(
            "application/vnd.tidal.bts",
            &json!({
                "codecs": codec,
                "encryptionType": encryption,
                "urls": ["https://example.test/audio"],
            })
            .to_string(),
            16,
            44100,
        )
    }

    // Exercise the real resolver and assert the requests it actually makes,
    // including the fallback order. No Tidal account or external network needed.
    async fn resolve(
        responses: Vec<(&'static str, u16, Value)>,
    ) -> Result<(String, DeliveredQuality)> {
        resolve_with_hires(responses, false, false).await
    }

    async fn resolve_with_hires(
        responses: Vec<(&'static str, u16, Value)>,
        second_login: bool,
        atmos: bool,
    ) -> Result<(String, DeliveredQuality)> {
        let path =
            std::env::temp_dir().join(format!("riptide-hires-{}.json", rand::random::<u64>()));
        if second_login {
            crate::api::auth::save_config_at(
                &Config {
                    access_token: Some("hires-test-token".into()),
                    expires_at: Some(
                        (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                    ),
                    user_id: Some(42),
                    country_code: "NL".into(),
                    session_id: Some("hires-session".into()),
                    ..Config::default()
                },
                &path,
            )
            .unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            timeout(Duration::from_secs(5), async move {
                for (quality, status, body) in responses {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut stream = BufReader::new(stream);
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    assert!(line.starts_with("GET /tracks/42/playbackinfopostpaywall?"));
                    assert!(line.contains(&format!("audioquality={quality}&")), "{line}");
                    assert!(line.contains(&format!("immersiveaudio={atmos}")));
                    let use_hires = second_login && !atmos && quality == "HI_RES_LOSSLESS";
                    if use_hires {
                        assert!(line.contains("sessionId=hires-session"));
                    }
                    let mut authorization = String::new();
                    loop {
                        line.clear();
                        stream.read_line(&mut line).await.unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                        if line.to_ascii_lowercase().starts_with("authorization:") {
                            authorization = line.split_once(':').unwrap().1.trim().to_owned();
                        }
                    }
                    assert_eq!(authorization, if use_hires { "Bearer hires-test-token" } else { "Bearer test-token" });
                    let body = body.to_string();
                    let response = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.get_mut().write_all(response.as_bytes()).await.unwrap();
                    stream.get_mut().shutdown().await.unwrap();
                }
            })
            .await
            .expect("resolver did not request the expected fallback");
        });
        let client = ApiClient::new(Config {
            access_token: Some("test-token".into()),
            user_id: Some(42),
            country_code: "NL".into(),
            ..Config::default()
        });
        let result = timeout(
            Duration::from_secs(5),
            client.get_stream_url_at(&base, 42, atmos, second_login.then_some(path.as_path())),
        )
        .await
        .expect("resolver timed out");
        server.await.unwrap();
        if second_login {
            std::fs::remove_file(path).unwrap();
        }
        result
    }

    #[tokio::test]
    async fn hires_login_is_used_for_hires_and_primary_login_for_lossless() {
        let (_, quality) = resolve_with_hires(
            vec![
                ("HI_RES_LOSSLESS", 200, bts("mp4a.40.2", "NONE")),
                ("LOSSLESS", 200, bts("flac", "NONE")),
            ],
            true,
            false,
        )
        .await
        .unwrap();
        assert_eq!(quality.bit_depth, Some(16));
    }

    #[tokio::test]
    async fn valid_hires_stream_from_second_login_needs_no_primary_request() {
        let mut response = bts("flac", "NONE");
        response["bitDepth"] = json!(24);
        response["sampleRate"] = json!(192000);
        let (_, quality) =
            resolve_with_hires(vec![("HI_RES_LOSSLESS", 200, response)], true, false)
                .await
                .unwrap();
        assert_eq!(quality.sample_rate, Some(192000));
        assert_eq!(quality.bit_depth, Some(24));
    }

    #[tokio::test]
    async fn rejected_hires_login_still_uses_primary_lossless() {
        resolve_with_hires(
            vec![
                ("HI_RES_LOSSLESS", 401, json!({})),
                ("LOSSLESS", 200, bts("flac", "NONE")),
            ],
            true,
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn atmos_keeps_using_primary_login() {
        resolve_with_hires(
            vec![("HI_RES_LOSSLESS", 200, bts("eac3", "NONE"))],
            true,
            true,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn plays_hires_dash_without_requesting_lower_quality() {
        let (url, quality) = resolve(vec![(
            "HI_RES_LOSSLESS",
            200,
            playback("application/dash+xml", DASH, 24, 96000),
        )])
        .await
        .unwrap();
        assert!(url.ends_with("/42.m3u8"));
        assert_eq!(quality.bit_depth, Some(24));
        assert_eq!(quality.sample_rate, Some(96000));
        let playlist = std::fs::read_to_string("/tmp/riptide_hls_42.m3u8").unwrap();
        std::fs::remove_file("/tmp/riptide_hls_42.m3u8").unwrap();
        assert!(playlist.contains("#EXT-X-MAP:URI=\"https://example.test/init.mp4\""));
        assert!(playlist.contains("https://example.test/2.m4s"));
    }

    #[tokio::test]
    async fn falls_back_to_lossless_for_unplayable_or_lossy_hires() {
        let manifests = [
            bts("mp4a.40.2", "NONE"),
            bts("flac", "OLD_AES"),
            playback("application/dash+xml", &DASH.replace("<Representation", r#"<ContentProtection value="cenc"/><Representation"#), 24, 96000),
            playback("application/dash+xml", &DASH.replace("<Representation", r#"<mpd:ContentProtection xmlns:mpd="urn:mpeg:dash:schema:mpd:2011" value="cenc"/><Representation"#), 24, 96000),
            playback("application/dash+xml", r#"<MPD><AdaptationSet codecs="flac"/></MPD>"#, 24, 96000),
            playback("application/dash+xml", r#"<MPD><AdaptationSet codecs="mp4a.40.2"/></MPD>"#, 16, 44100),
        ];
        for manifest in manifests {
            let (url, quality) = resolve(vec![
                ("HI_RES_LOSSLESS", 200, manifest),
                ("LOSSLESS", 200, bts("flac", "NONE")),
            ])
            .await
            .unwrap();
            assert_eq!(url, "https://example.test/audio");
            assert_eq!(quality.bit_depth, Some(16));
            assert_eq!(quality.sample_rate, Some(44100));
        }
    }

    #[tokio::test]
    async fn uses_aac_only_after_both_lossless_tiers_are_unavailable() {
        let (url, _) = resolve(vec![
            ("HI_RES_LOSSLESS", 403, json!({})),
            ("LOSSLESS", 403, json!({})),
            ("HIGH", 200, bts("mp4a.40.2", "NONE")),
        ])
        .await
        .unwrap();
        assert_eq!(url, "https://example.test/audio");
    }

    #[tokio::test]
    async fn unavailable_hires_asset_401_falls_back_without_reauthentication() {
        let (url, quality) = resolve(vec![
            (
                "HI_RES_LOSSLESS",
                401,
                json!({"status": 401, "subStatus": 4005, "userMessage": "Asset is not ready for playback"}),
            ),
            ("LOSSLESS", 200, bts("flac", "NONE")),
        ])
        .await
        .unwrap();
        assert_eq!(url, "https://example.test/audio");
        assert_eq!(quality.sample_rate, Some(44100));
    }

    #[tokio::test]
    async fn unavailable_asset_at_every_tier_reports_availability_not_login() {
        let error = resolve(
            ["HI_RES_LOSSLESS", "LOSSLESS", "HIGH"]
                .into_iter()
                .map(|quality| {
                    (
                        quality,
                        401,
                        json!({"status": 401, "subStatus": 4005, "userMessage": "Asset is not ready for playback"}),
                    )
                })
                .collect(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("No playable quality available"));
        assert!(error.contains("Asset is not ready for playback"));
        assert!(!error.contains("restart riptide"));
    }

    #[tokio::test]
    async fn expired_session_stops_without_trying_lower_quality() {
        let error = resolve(vec![("HI_RES_LOSSLESS", 401, json!({}))])
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Tidal rejected the session (401)")
        );
    }
}
