// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The colours the UI draws its accents and highlights in, and where they come from.

use ratatui::style::Color;

pub mod album;
pub mod catalog;
pub mod theme_file;

/// The roles a theme recolours. Body text and status colours are not among
/// them: body text follows the terminal's foreground, and red or green carry a
/// meaning a palette must not repaint.
///
/// The highlight and select colours are true colour in every palette. Their
/// background has to be — a palette entry would follow the terminal theme and
/// could land anywhere — and once it is fixed, every colour drawn over it must
/// be fixed too. `Color::White` is palette index 15, which light themes remap to
/// their *dark* text colour, so pairing it with a dark background gives
/// dark-on-dark; `Color::Cyan` and `Color::DarkGray` are remapped the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub accent: Color,
    pub dim: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
    pub highlight_dim: Color,
    pub highlight_accent: Color,
    pub select_bg: Color,
    pub select_fg: Color,
    /// Only fixed themes paint the window; `None` leaves the terminal's own
    /// background showing, which is what Default and Album promise.
    pub background: Option<Color>,
    pub text: Color,
    /// Split from `dim` because a theme's secondary-text colour can be loud —
    /// Cyberpunk's is magenta — and right for a row's second line but wrong on
    /// every border.
    pub border: Color,
    pub error: Color,
    pub success: Color,
    /// The artist biography: a paragraph, so softer than body text but far
    /// brighter than `dim`.
    pub bio: Color,
}

impl Palette {
    pub const DEFAULT: Palette = Palette {
        accent: Color::Cyan,
        dim: Color::DarkGray,
        highlight_bg: Color::Rgb(40, 40, 55),
        highlight_fg: Color::Rgb(236, 236, 245),
        highlight_dim: Color::Rgb(150, 150, 170),
        highlight_accent: Color::Rgb(120, 210, 232),
        select_bg: Color::Rgb(30, 100, 200),
        select_fg: Color::Rgb(236, 236, 245),
        background: None,
        text: Color::White,
        border: Color::DarkGray,
        error: Color::Red,
        success: Color::Green,
        bio: Color::Rgb(180, 180, 180),
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl std::str::FromStr for Rgb {
    type Err = String;

    fn from_str(hex: &str) -> Result<Self, String> {
        let invalid = || format!("{hex:?} is not #rrggbb");
        let digits = hex
            .strip_prefix('#')
            .filter(|d| d.len() == 6 && d.is_ascii())
            .ok_or_else(invalid)?;
        let channel =
            |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).map_err(|_| invalid());
        Ok(Rgb(channel(0)?, channel(2)?, channel(4)?))
    }
}

impl<'de> serde::Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        hex.parse().map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Rgb(r, g, b) = self;
        serializer.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
    }
}

impl From<Rgb> for Color {
    fn from(Rgb(r, g, b): Rgb) -> Self {
        Color::Rgb(r, g, b)
    }
}

/// The Material roles Riptide reads from a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Roles {
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub primary_container: Rgb,
    pub on_primary_container: Rgb,
    pub outline: Rgb,
    pub on_surface_variant: Rgb,
}

impl Palette {
    /// Material's container and on-container roles are designed as contrast
    /// pairs, so the selection bar takes them together rather than mixing one
    /// with a colour picked for some other surface.
    pub fn from_roles(roles: &Roles) -> Palette {
        Palette {
            accent: roles.primary.into(),
            dim: roles.outline.into(),
            highlight_bg: roles.primary_container.into(),
            highlight_fg: roles.on_primary_container.into(),
            highlight_dim: roles.on_surface_variant.into(),
            highlight_accent: roles.primary.into(),
            select_bg: roles.primary.into(),
            select_fg: roles.on_primary.into(),
            border: roles.outline.into(),
            ..Palette::DEFAULT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palette_is_todays_look() {
        let p = Palette::DEFAULT;
        assert_eq!(p.accent, Color::Cyan);
        assert_eq!(p.dim, Color::DarkGray);
        assert_eq!(p.highlight_bg, Color::Rgb(40, 40, 55));
        assert_eq!(p.highlight_fg, Color::Rgb(236, 236, 245));
        assert_eq!(p.highlight_dim, Color::Rgb(150, 150, 170));
        assert_eq!(p.highlight_accent, Color::Rgb(120, 210, 232));
        assert_eq!(p.select_bg, Color::Rgb(30, 100, 200));
        assert_eq!(p.select_fg, Color::Rgb(236, 236, 245));
        assert_eq!(p.background, None);
        assert_eq!(p.text, Color::White);
        assert_eq!(p.border, Color::DarkGray);
        assert_eq!(p.error, Color::Red);
        assert_eq!(p.success, Color::Green);
        assert_eq!(p.bio, Color::Rgb(180, 180, 180));
    }
}
