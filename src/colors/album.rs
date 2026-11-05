// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! A palette taken from the playing track's cover art.
//!
//! Uses Material's dynamic-colour algorithm (the one matugen and Android use),
//! whose container and on-container roles come as contrast pairs — which is
//! what keeps the selection bar legible on any cover.

use image::imageops::FilterType;
use material_colors::{
    color::Argb,
    quantize::{Quantizer, QuantizerCelebi},
    scheme::Scheme,
    score::Score,
    theme::ThemeBuilder,
};

use super::{Palette, Rgb, Roles};

/// The dominant colours survive a heavy downscale. Measured in a release build
/// on a busy 320px cover: 26 ms at 128px, 8 ms at 64px. It runs on the UI
/// thread at track change, so 64px keeps it inside a single frame.
const SAMPLE_EDGE: u32 = 64;

/// Material's own colour budget for image extraction.
const MAX_COLORS: usize = 128;

/// `None` when the bytes are not a decodable image.
pub fn palette_from_cover(bytes: &[u8], dark: bool) -> Option<Palette> {
    let started = std::time::Instant::now();
    let image = image::load_from_memory(bytes).ok()?;
    let pixels: Vec<Argb> = image
        .resize(SAMPLE_EDGE, SAMPLE_EDGE, FilterType::Triangle)
        .into_rgb8()
        .pixels()
        .map(|p| Argb::new(255, p[0], p[1], p[2]))
        .collect();
    let quantized = QuantizerCelebi::quantize(&pixels, MAX_COLORS);
    let seed = Score::score(&quantized.color_to_count, None, None, None)[0];
    let schemes = ThemeBuilder::with_source(seed).build().schemes;
    let scheme = if dark { schemes.dark } else { schemes.light };
    tracing::debug!(
        "theme: cover palette in {:.1} ms (seed {})",
        started.elapsed().as_secs_f64() * 1000.0,
        seed.to_hex_with_pound()
    );
    Some(Palette::from_roles(&roles(&scheme)))
}

fn roles(scheme: &Scheme) -> Roles {
    let rgb = |c: Argb| Rgb(c.red, c.green, c.blue);
    Roles {
        primary: rgb(scheme.primary),
        on_primary: rgb(scheme.on_primary),
        primary_container: rgb(scheme.primary_container),
        on_primary_container: rgb(scheme.on_primary_container),
        outline: rgb(scheme.outline),
        on_surface_variant: rgb(scheme.on_surface_variant),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::solid_png;
    use ratatui::style::Color;

    #[test]
    fn warm_cover_gives_a_warm_accent() {
        let palette = palette_from_cover(&solid_png(0xd2, 0x69, 0x1e), true).unwrap();
        let Color::Rgb(r, _, b) = palette.accent else {
            panic!("accent is not true colour: {:?}", palette.accent);
        };
        assert!(r > b, "accent {:?} is not warm", palette.accent);
    }

    #[test]
    fn cover_palette_is_true_colour_throughout() {
        let p = palette_from_cover(&solid_png(0x2e, 0x8b, 0x57), true).unwrap();
        for color in [
            p.accent,
            p.dim,
            p.highlight_bg,
            p.highlight_fg,
            p.highlight_dim,
            p.highlight_accent,
            p.select_bg,
            p.select_fg,
        ] {
            assert!(
                matches!(color, Color::Rgb(..)),
                "{color:?} is not true colour"
            );
        }
    }

    #[test]
    fn light_and_dark_schemes_differ() {
        let png = solid_png(0xd2, 0x69, 0x1e);
        assert_ne!(
            palette_from_cover(&png, true),
            palette_from_cover(&png, false)
        );
    }

    #[test]
    fn grey_cover_still_gives_a_palette() {
        let palette = palette_from_cover(&solid_png(0x80, 0x80, 0x80), true).unwrap();
        assert_ne!(palette.highlight_bg, palette.highlight_fg);
    }

    #[test]
    fn garbage_bytes_give_none() {
        assert_eq!(palette_from_cover(b"not an image", true), None);
        assert_eq!(palette_from_cover(&[], true), None);
    }
}
