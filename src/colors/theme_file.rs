// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Theme files: a name and Catppuccin's colour roles, mapped onto a [`Palette`].
//!
//! Catppuccin's vocabulary rather than one of our own, so a theme can be
//! copied from any of the many ports already published in it. All 22 roles are
//! required even though only some are drawn today: a file that parses now keeps
//! parsing when a later role starts using another one.

use super::{Palette, Rgb};

#[derive(Debug, Clone)]
pub struct ThemeFile {
    pub name: String,
    pub palette: Palette,
}

#[derive(serde::Deserialize)]
struct Raw {
    name: String,
    colors: Colors,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct Colors {
    base: Rgb,
    mantle: Rgb,
    crust: Rgb,
    text: Rgb,
    subtext0: Rgb,
    subtext1: Rgb,
    surface0: Rgb,
    surface1: Rgb,
    surface2: Rgb,
    overlay0: Rgb,
    overlay1: Rgb,
    overlay2: Rgb,
    blue: Rgb,
    sapphire: Rgb,
    peach: Rgb,
    green: Rgb,
    red: Rgb,
    mauve: Rgb,
    pink: Rgb,
    yellow: Rgb,
    maroon: Rgb,
    teal: Rgb,
}

/// The ten built-ins, in the order the picker lists them. Compiled in because
/// no package installs `assets/`.
const BUILTIN: [&str; 10] = [
    include_str!("../../assets/themes/catppuccin-latte.json"),
    include_str!("../../assets/themes/catppuccin-frappe.json"),
    include_str!("../../assets/themes/catppuccin-macchiato.json"),
    include_str!("../../assets/themes/catppuccin-mocha.json"),
    include_str!("../../assets/themes/nord.json"),
    include_str!("../../assets/themes/gruvbox-dark.json"),
    include_str!("../../assets/themes/tokyo-night.json"),
    include_str!("../../assets/themes/dracula.json"),
    include_str!("../../assets/themes/rose-pine.json"),
    include_str!("../../assets/themes/cyberpunk.json"),
];

/// A missing or malformed colour rejects the whole file: drawing a theme with
/// a hole in it would put a terminal palette entry beside true colours.
pub fn parse(json: &str) -> anyhow::Result<ThemeFile> {
    let raw: Raw = serde_json::from_str(json)?;
    let c = raw.colors;
    Ok(ThemeFile {
        name: raw.name,
        palette: Palette {
            accent: c.mauve.into(),
            dim: c.subtext0.into(),
            highlight_bg: c.surface1.into(),
            highlight_fg: c.text.into(),
            highlight_dim: c.subtext1.into(),
            highlight_accent: c.mauve.into(),
            select_bg: c.mauve.into(),
            select_fg: c.base.into(),
            background: Some(c.base.into()),
            text: c.text.into(),
            border: c.overlay0.into(),
            error: c.red.into(),
            success: c.green.into(),
            bio: c.subtext1.into(),
        },
    })
}

pub fn builtins() -> Vec<ThemeFile> {
    BUILTIN
        .iter()
        .map(|json| parse(json).expect("built-in theme files are checked by a test"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    const CYBERPUNK: &str = include_str!("../../assets/themes/cyberpunk.json");

    #[test]
    fn cyberpunk_parses_with_its_exact_values() {
        let theme = parse(CYBERPUNK).unwrap();
        assert_eq!(theme.name, "Cyberpunk");
        let p = theme.palette;
        assert_eq!(p.background, Some(Color::Rgb(0x10, 0x10, 0x14)));
        assert_eq!(p.text, Color::Rgb(0x00, 0xf0, 0xff));
        assert_eq!(p.dim, Color::Rgb(0xd0, 0x00, 0x6f));
        assert_eq!(p.border, Color::Rgb(0x4d, 0x4d, 0x6c));
        assert_eq!(p.accent, Color::Rgb(0xe6, 0x00, 0xff));
        assert_eq!(p.highlight_bg, Color::Rgb(0x26, 0x26, 0x36));
        assert_eq!(p.select_fg, Color::Rgb(0x10, 0x10, 0x14));
        assert_eq!(p.error, Color::Rgb(0xff, 0x00, 0x55));
        assert_eq!(p.success, Color::Rgb(0x00, 0xff, 0x66));
    }

    #[test]
    fn missing_colour_rejects_the_file() {
        let without = CYBERPUNK.replace(r##""teal": "#00f0ff""##, r##""x": "#000000""##);
        assert_ne!(without, CYBERPUNK);
        assert!(parse(&without).is_err());
    }

    #[test]
    fn bad_hex_rejects_the_file() {
        let bad = CYBERPUNK.replace(r##""red": "#ff0055""##, r##""red": "red""##);
        assert_ne!(bad, CYBERPUNK);
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn missing_name_rejects_the_file() {
        let nameless = CYBERPUNK.replace(r#""name": "Cyberpunk","#, "");
        assert_ne!(nameless, CYBERPUNK);
        assert!(parse(&nameless).is_err());
    }

    #[test]
    fn every_builtin_parses_and_has_a_unique_name() {
        let names: Vec<String> = builtins().iter().map(|t| t.name.to_lowercase()).collect();
        assert_eq!(
            names,
            [
                "catppuccin latte",
                "catppuccin frappé",
                "catppuccin macchiato",
                "catppuccin mocha",
                "nord",
                "gruvbox dark",
                "tokyo night",
                "dracula",
                "rosé pine",
                "cyberpunk",
            ]
        );
    }
}
