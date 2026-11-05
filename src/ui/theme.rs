// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Colours, spacing constants and small formatting helpers.

use std::cell::Cell;

use ratatui::style::{Modifier, Style};

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

/// Blank `area` for an overlay. `Clear` alone resets cells to the terminal's
/// default, which under a fixed theme punches a hole of terminal colour
/// through the window, so the theme's background goes straight back on.
pub(super) fn clear_area(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    f.render_widget(ratatui::widgets::Clear, area);
    paint_background(f, area);
}

pub(super) fn paint_background(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    if let Some(bg) = palette().background {
        f.render_widget(
            ratatui::widgets::Block::default().style(Style::default().bg(bg)),
            area,
        );
    }
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
        Style::default().fg(palette().text)
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

/// The quality badge and the now-playing marker: the cells that keep their own
/// colour on every row.
/// The now-playing marker over a row's own style, so it keeps that row's
/// background — a cursor bar, or a held row's reversed one — and changes only
/// its colour.
pub(super) fn marker_style(row: Style) -> Style {
    let accent = if row.bg.is_some() {
        palette().highlight_accent
    } else {
        palette().accent
    };
    row.fg(accent).add_modifier(Modifier::BOLD)
}

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
