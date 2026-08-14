// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Terminal image rendering.
//!
//! Protocols are cached by (content, size, render generation); see
//! PROTOCOL_CACHE for why that
//! matters for render latency.

use ratatui::layout::{Rect, Size};
use ratatui::Frame;
use ratatui_image::{picker::Picker, protocol::Protocol, FilterType, Image, Resize};

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
    render_generation: u64,
}

thread_local! {
    /// Cached terminal-image protocols, keyed by image content, target size,
    /// resize behavior, and an explicit render generation.
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
    /// diff emits nothing at all once the image has been sent. Fullscreen art
    /// advances its generation after a modal closes: Kitty placeholders that
    /// were overwritten by the modal then get a new protocol id and the full
    /// affected rows are emitted again.
    static PROTOCOL_CACHE: std::cell::RefCell<
        std::collections::HashMap<ProtocolCacheKey, Protocol>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

// Initialize picker once at startup to avoid blocking on every frame
pub(super) fn get_picker() -> &'static Picker {
    static PICKER: std::sync::OnceLock<Picker> = std::sync::OnceLock::new();
    PICKER.get_or_init(|| {
        let term = std::env::var("TERM").unwrap_or_else(|_| "unknown".to_string());
        let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "not set".to_string());
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
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

pub(super) fn release_scaled_protocols() {
    PROTOCOL_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .retain(|key, _| key.resize != ImageResize::Scale);
    });
}

pub(super) fn render_image(f: &mut Frame, bytes: &[u8], area: Rect) {
    render_image_with_resize(f, bytes, area, ImageResize::Fit, false, 0);
}

pub(super) fn render_scaled_image(
    f: &mut Frame,
    bytes: &[u8],
    area: Rect,
    render_generation: u64,
) {
    render_image_with_resize(
        f,
        bytes,
        area,
        ImageResize::Scale,
        true,
        render_generation,
    );
}

fn render_image_with_resize(
    f: &mut Frame,
    bytes: &[u8],
    area: Rect,
    resize: ImageResize,
    center: bool,
    render_generation: u64,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Hash the content rather than keying on the slice's address: art buffers
    // are freed and reallocated as the user browses, and an allocator reusing
    // an address for a same-sized image would otherwise serve a stale picture.
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        ProtocolCacheKey {
            content_hash: hasher.finish(),
            width: area.width,
            height: area.height,
            resize,
            render_generation,
        }
    };

    PROTOCOL_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();

        if !cache.contains_key(&key) {
            let Ok(img) = image::load_from_memory(bytes) else {
                return;
            };

            // A scaled Kitty protocol can retain a multi-megabyte RGBA
            // transmission. Fullscreen redraw generations and track changes
            // supersede one another, so keeping more than the current one can
            // multiply RSS without making a future frame faster.
            prune_superseded_scaled_protocols(&mut cache, key);
            if cache.len() >= PROTOCOL_CACHE_CAP {
                cache.clear();
            }

            let resize = match resize {
                ImageResize::Fit => Resize::Fit(None),
                ImageResize::Scale => Resize::Scale(Some(FilterType::CatmullRom)),
            };
            let Ok(protocol) = get_picker().new_protocol(img, area.into(), resize) else {
                return;
            };
            cache.insert(key, protocol);
        }

        if let Some(protocol) = cache.get(&key) {
            let render_area = if center {
                centered_protocol_area(area, protocol.size())
            } else {
                area
            };
            f.render_widget(Image::new(protocol), render_area);
        }
    });
}

fn prune_superseded_scaled_protocols<T>(
    cache: &mut std::collections::HashMap<ProtocolCacheKey, T>,
    requested: ProtocolCacheKey,
) {
    if requested.resize == ImageResize::Scale {
        cache.retain(|existing, _| {
            existing.resize != ImageResize::Scale || *existing == requested
        });
    }
}

fn centered_protocol_area(area: Rect, size: Size) -> Rect {
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
    fn scaled_protocols_use_one_replaceable_cache_slot() {
        fn key(content_hash: u64, resize: ImageResize, render_generation: u64) -> ProtocolCacheKey {
            ProtocolCacheKey {
                content_hash,
                width: 80,
                height: 40,
                resize,
                render_generation,
            }
        }

        let fit = key(1, ImageResize::Fit, 0);
        let old_scale = key(2, ImageResize::Scale, 0);
        let new_scale = key(2, ImageResize::Scale, 1);
        let mut cache = std::collections::HashMap::from([(fit, ()), (old_scale, ())]);

        prune_superseded_scaled_protocols(&mut cache, new_scale);
        cache.insert(new_scale, ());

        assert!(cache.contains_key(&fit));
        assert!(!cache.contains_key(&old_scale));
        assert!(cache.contains_key(&new_scale));
        assert_eq!(
            cache
                .keys()
                .filter(|key| key.resize == ImageResize::Scale)
                .count(),
            1,
        );
    }
}
