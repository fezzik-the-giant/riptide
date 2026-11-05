// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Colours, spacing constants and small formatting helpers.

use std::cell::Cell;

use ratatui::style::{Color, Modifier, Style};

pub(super) use crate::colors::Palette;

thread_local! {
    /// The palette this frame draws in, installed by `draw`.
    ///
    /// A thread-local rather than a parameter because `row_style` and its
    /// siblings are called from dozens of sites that hold no `&App`; threading
    /// a palette through each would change every row builder's signature for a
    /// value that is constant across the frame.
    static ACTIVE: Cell<Palette> = const { Cell::new(Palette::DEFAULT) };
}

pub(super) fn palette() -> Palette {
    ACTIVE.with(Cell::get)
}

pub(super) fn set_palette(palette: Palette) {
    ACTIVE.with(|active| active.set(palette));
}

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
            .bg(palette().highlight_bg)
            .fg(palette().highlight_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

/// Secondary columns — year, track count, the artist beside an album title.
pub(super) fn row_dim_style(is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .bg(palette().highlight_bg)
            .fg(palette().highlight_dim)
    } else {
        Style::default().fg(palette().dim)
    }
}

/// The quality badge, the one cell that keeps its own colour on every row.
pub(super) fn row_accent_style(is_selected: bool) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if is_selected {
        style
            .bg(palette().highlight_bg)
            .fg(palette().highlight_accent)
    } else {
        style.fg(palette().accent)
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
