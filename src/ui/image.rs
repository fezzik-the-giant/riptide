// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Terminal image rendering.
//!
//! Protocols are cached by content, size, and resize behavior; see
//! [`PROTOCOL_CACHE`] for why that matters for render latency.

use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::{FilterType, Image, Resize, picker::Picker, protocol::Protocol};

use crate::app::{CachedImage, image_content_hash};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum ImageResize {
    Fit,
    Scale,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct ProtocolCacheKey {
    content_hash: u64,
    width: u16,
    height: u16,
    resize: ImageResize,
}

#[derive(Clone, Copy, Default)]
struct ImageFrameState {
    fullscreen_art: bool,
    overlay_active: bool,
}

thread_local! {
    /// Cached terminal-image protocols, keyed by image content, target size,
    /// and resize behavior.
    ///
    /// Building a protocol decodes the source image and re-encodes it for the
    /// terminal's graphics protocol — ~760 µs for a 320x320 JPEG under Kitty.
    /// Worse, every rebuild produces a *different* payload (a fresh image id),
    /// so ratatui's buffer diff cannot skip it and the whole ~135 KiB escape
    /// sequence is written to the terminal again. With two images on screen —
    /// the now-playing art plus a detail view's art — that came to ~16 MiB/s at
    /// 60 fps, which is what made the cursor lag in the artist, album and
    /// playlist views.
    ///
    /// Cached protocols render byte-identical cells on repeat frames, so the
    /// diff emits nothing at all once the image has been sent.
    static PROTOCOL_CACHE: std::cell::RefCell<
        std::collections::HashMap<ProtocolCacheKey, Option<Protocol>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());

    /// Tracks renderer-visible transitions that invalidate the scaled Kitty
    /// placement. Keeping this beside the cache means callers do not need to
    /// know when a terminal image protocol must be rebuilt.
    static IMAGE_FRAME_STATE: std::cell::Cell<ImageFrameState> =
        const { std::cell::Cell::new(ImageFrameState {
            fullscreen_art: false,
            overlay_active: false,
        }) };
}

// Initialize picker once at startup to avoid blocking on every frame
pub(super) fn get_picker() -> &'static Picker {
    static PICKER: std::sync::OnceLock<Picker> = std::sync::OnceLock::new();
    PICKER.get_or_init(|| {
        let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".to_string());
        let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
        let picker = match Picker::from_query_stdio() {
            Ok(picker) => picker,
            Err(error) => {
                tracing::warn!(%error, "failed to detect terminal image protocol; using halfblocks");
                Picker::halfblocks()
            }
        };
        tracing::info!(
            "Terminal: TERM={}, COLORTERM={} → Image protocol: {:?}",
            term,
            colorterm,
            picker.protocol_type()
        );
        picker
    })
}

/// Only a couple of (image, size) pairs are ever live at once, so a small cap
/// with a wholesale clear is enough to bound growth as the user browses.
pub(super) const PROTOCOL_CACHE_CAP: usize = 8;

fn release_scaled_protocols() {
    PROTOCOL_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .retain(|key, _| key.resize != ImageResize::Scale);
    });
}

pub(super) fn prepare_image_frame(fullscreen_art: bool, overlay_active: bool) {
    let next = ImageFrameState {
        fullscreen_art,
        overlay_active,
    };
    let release_scaled = IMAGE_FRAME_STATE.with(|state| {
        let previous = state.replace(next);
        should_release_scaled_protocol(previous, next)
    });

    if release_scaled {
        release_scaled_protocols();
    }
}

fn should_release_scaled_protocol(previous: ImageFrameState, next: ImageFrameState) -> bool {
    // Closing an overlay exposes cells that the overlay overwrote. Rebuilding
    // the virtual placement makes the terminal paint those cells again.
    (previous.fullscreen_art && !next.fullscreen_art)
        || (next.fullscreen_art && previous.overlay_active && !next.overlay_active)
}

pub(super) fn render_image(f: &mut Frame, bytes: &[u8], area: Rect) -> bool {
    render_image_with_resize(f, bytes, area, ImageResize::Fit, image_content_hash(bytes))
}

pub(super) fn render_scaled_image(f: &mut Frame, image: &CachedImage, area: Rect) -> bool {
    render_image_with_resize(
        f,
        image.bytes(),
        area,
        ImageResize::Scale,
        image.content_hash(),
    )
}

fn render_image_with_resize(
    f: &mut Frame,
    bytes: &[u8],
    area: Rect,
    resize: ImageResize,
    content_hash: u64,
) -> bool {
    if area.width == 0 || area.height == 0 {
        return false;
    }

    let key = ProtocolCacheKey {
        content_hash,
        width: area.width,
        height: area.height,
        resize,
    };

    PROTOCOL_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        if !cache.contains_key(&key) {
            // A scaled Kitty protocol can retain a multi-megabyte RGBA
            // transmission. Fullscreen redraws and track changes supersede one
            // another, so keeping more than the current one can
            // multiply RSS without making a future frame faster.
            prune_superseded_scaled_protocols(&mut cache, key);
            if cache.len() >= PROTOCOL_CACHE_CAP {
                cache.clear();
            }

            let img = match image::load_from_memory(bytes) {
                Ok(img) => img,
                Err(error) => {
                    tracing::warn!(%error, bytes = bytes.len(), "failed to decode artwork");
                    // Cache the failure so corrupt bytes do not get decoded and
                    // logged again on every draw tick.
                    cache.insert(key, None);
                    return false;
                }
            };
            let protocol_resize = match resize {
                ImageResize::Fit => Resize::Fit(None),
                ImageResize::Scale => Resize::Scale(Some(FilterType::CatmullRom)),
            };
            let protocol = match get_picker().new_protocol(img, area.into(), protocol_resize) {
                Ok(protocol) => protocol,
                Err(error) => {
                    tracing::warn!(%error, "failed to prepare artwork for the terminal");
                    cache.insert(key, None);
                    return false;
                }
            };
            cache.insert(key, Some(protocol));
        }

        let Some(Some(protocol)) = cache.get(&key) else {
            return false;
        };
        let render_area = match resize {
            ImageResize::Fit => area,
            ImageResize::Scale => centered_protocol_area(area, protocol.size()),
        };
        f.render_widget(Image::new(protocol), render_area);
        true
    })
}

fn prune_superseded_scaled_protocols<T>(
    cache: &mut std::collections::HashMap<ProtocolCacheKey, Option<T>>,
    requested: ProtocolCacheKey,
) {
    if requested.resize == ImageResize::Scale {
        cache.retain(|existing, protocol| {
            existing.resize != ImageResize::Scale || *existing == requested || protocol.is_none()
        });
    }
}

pub(super) fn centered_protocol_area(area: Rect, size: Size) -> Rect {
    let width = size.width.min(area.width);
    let height = size.height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use ratatui_image::FontSize;

    #[test]
    fn scale_upfills_a_target_that_fit_leaves_at_natural_size() {
        let image = image::DynamicImage::new_rgba8(320, 320);
        let font_size = FontSize::new(8, 16);
        let target = Size::new(80, 40);

        assert_eq!(
            Resize::Fit(None).size_for(&image, font_size, target),
            Size::new(40, 20)
        );
        assert_eq!(
            Resize::Scale(None).size_for(&image, font_size, target),
            target
        );
    }

    #[test]
    fn actual_protocol_size_is_centered_after_cell_rounding() {
        assert_eq!(
            centered_protocol_area(Rect::new(10, 5, 80, 40), Size::new(76, 38)),
            Rect::new(12, 6, 76, 38),
        );
    }

    #[test]
    fn scaled_protocols_use_one_live_cache_slot_and_keep_failures() {
        fn key(content_hash: u64, resize: ImageResize) -> ProtocolCacheKey {
            ProtocolCacheKey {
                content_hash,
                width: 80,
                height: 40,
                resize,
            }
        }

        let fit = key(1, ImageResize::Fit);
        let old_scale = key(2, ImageResize::Scale);
        let new_scale = key(3, ImageResize::Scale);
        let failed_scale = key(4, ImageResize::Scale);
        let mut cache = std::collections::HashMap::from([
            (fit, Some(())),
            (old_scale, Some(())),
            (failed_scale, None),
        ]);

        prune_superseded_scaled_protocols(&mut cache, new_scale);
        cache.insert(new_scale, Some(()));

        assert!(cache.contains_key(&fit));
        assert!(!cache.contains_key(&old_scale));
        assert!(cache.contains_key(&new_scale));
        assert!(matches!(cache.get(&failed_scale), Some(None)));
        assert_eq!(
            cache
                .keys()
                .filter(|key| {
                    key.resize == ImageResize::Scale && cache.get(key).is_some_and(Option::is_some)
                })
                .count(),
            1,
        );
    }

    #[test]
    fn scaled_protocol_is_released_after_overlay_or_fullscreen_closes() {
        let normal = ImageFrameState::default();
        let art = ImageFrameState {
            fullscreen_art: true,
            overlay_active: false,
        };
        let art_with_overlay = ImageFrameState {
            fullscreen_art: true,
            overlay_active: true,
        };

        assert!(!should_release_scaled_protocol(normal, art));
        assert!(!should_release_scaled_protocol(art, art_with_overlay));
        assert!(should_release_scaled_protocol(art_with_overlay, art));
        assert!(should_release_scaled_protocol(art, normal));
    }

    #[test]
    fn invalid_image_failure_is_cached_and_reported() {
        let image = CachedImage::new(b"not an image".to_vec());
        let area = Rect::new(0, 0, 10, 5);
        let key = ProtocolCacheKey {
            content_hash: image.content_hash(),
            width: area.width,
            height: area.height,
            resize: ImageResize::Scale,
        };
        PROTOCOL_CACHE.with(|cache| cache.borrow_mut().clear());

        let mut rendered = true;
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|f| rendered = render_scaled_image(f, &image, area))
            .unwrap();

        assert!(!rendered);
        PROTOCOL_CACHE.with(|cache| {
            assert!(matches!(cache.borrow().get(&key), Some(None)));
            cache.borrow_mut().clear();
        });
    }
}
