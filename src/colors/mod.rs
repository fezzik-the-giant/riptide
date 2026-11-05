// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The colours the UI draws its accents and highlights in, and where they come from.

use ratatui::style::Color;

pub mod album;

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
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl From<Rgb> for Color {
    fn from(Rgb(r, g, b): Rgb) -> Self {
        Color::Rgb(r, g, b)
    }
}

/// The Material roles Riptide reads from a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        }
    }
}

/// Where the palette comes from, as chosen in Settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSource {
    Album,
    /// `other` so a config written by a newer version with a source this one
    /// lacks still loads, rather than failing the whole preferences block.
    #[default]
    #[serde(other)]
    Default,
}

impl ThemeSource {
    const CYCLE: [ThemeSource; 2] = [Self::Default, Self::Album];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Album => "Album",
        }
    }

    pub fn next(self) -> Self {
        self.step(1)
    }

    pub fn previous(self) -> Self {
        self.step(Self::CYCLE.len() - 1)
    }

    fn step(self, by: usize) -> Self {
        let at = Self::CYCLE.iter().position(|&s| s == self).unwrap_or(0);
        Self::CYCLE[(at + by) % Self::CYCLE.len()]
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
    }

    #[test]
    fn theme_source_cycles_and_wraps() {
        assert_eq!(ThemeSource::Default.next(), ThemeSource::Album);
        assert_eq!(ThemeSource::Album.next(), ThemeSource::Default);
        assert_eq!(ThemeSource::Album.previous(), ThemeSource::Default);
    }

    #[test]
    fn unknown_theme_source_loads_as_default() {
        let source: ThemeSource = serde_json::from_str("\"neon\"").unwrap();
        assert_eq!(source, ThemeSource::Default);
    }
}
