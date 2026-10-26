// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Colours, spacing constants and small formatting helpers.

use ratatui::style::{Color, Modifier, Style};

pub(super) const ACCENT: Color = Color::Cyan;

pub(super) const DIM: Color = Color::DarkGray;

pub(super) fn fmt_sample_rate(hz: u32) -> String {
    match hz {
        44100 => "44.1 kHz".into(),
        88200 => "88.2 kHz".into(),
        176400 => "176.4 kHz".into(),
        _ => {
            let khz = hz / 1000;
            format!("{khz} kHz")
        }
    }
}

/// Selected-row colours.
///
/// All four are true colour on purpose. The background has to be — a palette
/// entry would follow the terminal theme and could land anywhere — and once it
/// is fixed, every colour drawn over it must be fixed too. `Color::White` is
/// palette index 15, which light themes remap to their *dark* text colour, so
/// pairing it with a dark background gives dark-on-dark; `Color::Cyan` and
/// `Color::DarkGray` are remapped the same way.
pub(super) const HIGHLIGHT_BG: Color = Color::Rgb(40, 40, 55);
pub(super) const HIGHLIGHT_FG: Color = Color::Rgb(236, 236, 245);
pub(super) const HIGHLIGHT_DIM: Color = Color::Rgb(150, 150, 170);
pub(super) const HIGHLIGHT_ACCENT: Color = Color::Rgb(120, 210, 232);

pub(super) const SELECT_BG: Color = Color::Rgb(30, 100, 200);
pub(super) const SELECT_FG: Color = Color::Rgb(236, 236, 245);

/// The style a list row is drawn in.
///
/// Every cell of a selected row must carry this background: `layout_row` pads
/// each cell to its full width, so one built from a bare `Style::default()`
/// paints its padding in the terminal's background and punches a gap through
/// the bar. Use [`row_dim_style`] and [`row_accent_style`] for cells that need
/// their own colour rather than starting a new style.
pub(super) fn row_style(is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .bg(HIGHLIGHT_BG)
            .fg(HIGHLIGHT_FG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

/// Secondary columns — year, track count, the artist beside an album title.
pub(super) fn row_dim_style(is_selected: bool) -> Style {
    if is_selected {
        Style::default().bg(HIGHLIGHT_BG).fg(HIGHLIGHT_DIM)
    } else {
        Style::default().fg(DIM)
    }
}

/// The quality badge, the one cell that keeps its own colour on every row.
pub(super) fn row_accent_style(is_selected: bool) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if is_selected {
        style.bg(HIGHLIGHT_BG).fg(HIGHLIGHT_ACCENT)
    } else {
        style.fg(ACCENT)
    }
}

pub(super) const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub(super) fn spinner_char(tick: u64) -> char {
    SPINNER[(tick / 3) as usize % SPINNER.len()]
}

/// Blinking block for text inputs. Shared so every input box blinks in step.
pub(super) fn cursor_char(tick: u64) -> &'static str {
    if (tick / 30).is_multiple_of(2) {
        "█"
    } else {
        " "
    }
}
