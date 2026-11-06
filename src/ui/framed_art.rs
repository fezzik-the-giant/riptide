// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Cover art inside a border that hugs it, shared by Home and the detail views.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    symbols::border,
    widgets::{Block, Borders, Paragraph},
};

use super::*;

/// Narrowest sidebar the album and playlist views give their metadata when the
/// art beside it has shrunk away.
pub(super) const DETAIL_SIDEBAR_MIN_W: u16 = 12;

/// Rows every art sidebar keeps under its cover for a title, metadata or bio.
const SIDEBAR_TEXT_MIN_H: u16 = 5;

/// Box-drawing lines run through the middle of their cells, leaving half a cell
/// of blank between the border and an image that fills the cells inside it.
/// One-eighth blocks drawn on the inner edge of each border cell close that
/// gap. The corners stay blank: no single glyph draws just their inner corner.
const SNUG_BORDER: border::Set = border::Set {
    top_left: " ",
    top_right: " ",
    bottom_left: " ",
    bottom_right: " ",
    vertical_left: border::ONE_EIGHTH_RIGHT_EIGHT,
    vertical_right: border::ONE_EIGHTH_LEFT_EIGHT,
    horizontal_top: border::ONE_EIGHTH_BOTTOM_EIGHT,
    horizontal_bottom: border::ONE_EIGHTH_TOP_EIGHT,
};

/// Where a view's sidebar art may go. Home and the artist, album and playlist
/// views all size their cover from this, so it is the same size in each.
pub(super) fn sidebar_art_bounds(area: Rect) -> Rect {
    Rect {
        width: area.width / 4,
        height: area.height.saturating_sub(SIDEBAR_TEXT_MIN_H),
        ..area
    }
}

/// The largest frame, border included, that fits in `bounds` at its top-left
/// corner around a square cover. Zero-sized when nothing fits.
pub(super) fn art_frame(bounds: Rect) -> Rect {
    let (cell_w, cell_h) = cell_size();
    let (cols, rows) = art_frame_cells(bounds, cell_w, cell_h).unwrap_or((0, 0));
    let (w, h) = if cols == 0 {
        (0, 0)
    } else {
        (cols + 2, rows + 2)
    };
    Rect::new(bounds.x, bounds.y, w, h)
}

/// Inner size of the art frame, in cells, or `None` when nothing fits.
///
/// Covers are square, so the frame has to be square in *pixels* — which takes
/// the terminal's real cell size, not an assumed one. Whole cells rarely divide
/// evenly, so the frame is fitted to the smaller axis: the result is square to
/// within one cell, not to the pixel. The cover is filled into it, cropping the
/// sliver that difference amounts to.
///
/// Split out from the rendering so it can be tested at cell sizes other than
/// whatever the machine running the tests happens to report.
fn art_frame_cells(bounds: Rect, cell_w: u16, cell_h: u16) -> Option<(u16, u16)> {
    let mut cols = bounds.width.saturating_sub(2);
    let mut rows = (cols as u32 * cell_w as u32 / cell_h as u32) as u16;
    rows = rows.min(bounds.height.saturating_sub(2));
    // Back-solve the width from the rows that survived rounding, so the frame is
    // the size the fitted image ends up rather than the size it asked for.
    cols = cols.min((rows as u32 * cell_h as u32 / cell_w as u32) as u16);
    (cols > 0 && rows > 0).then_some((cols, rows))
}

/// Draws the border and the cover inside `frame` (from [`art_frame`]), or a
/// spinner while the cover is still on its way.
pub(super) fn render_framed_art(
    f: &mut Frame,
    frame: Rect,
    bytes: Option<&[u8]>,
    loading: bool,
    tick: u64,
    border_color: Color,
) {
    if frame.is_empty() {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(SNUG_BORDER)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(frame);
    f.render_widget(block, frame);

    if let Some(bytes) = bytes {
        render_filled_image(f, bytes, inner);
    } else if loading {
        f.render_widget(
            Paragraph::new(spinner_char(tick).to_string())
                .style(Style::default().fg(palette().dim))
                .alignment(Alignment::Center),
            inner,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame is laid out in whole cells, so it can only be square to within
    /// one of them: `cell_size()` reports the terminal's real font metrics, and
    /// the 1:2 ratio the arithmetic would need to land exactly is not what most
    /// terminals use.
    ///
    /// This asserted exact pixel equality once, which made it a test of the host
    /// rather than of the code — it passed wherever no terminal answered the
    /// query and `Picker::halfblocks()` supplied its 10x20 fallback (CI, COPR),
    /// and failed in every AUR build run from a terminal with, say, 9x20 cells.
    #[test]
    fn the_art_frame_is_square_to_within_one_cell() {
        for (cell_w, cell_h) in [(10u16, 20u16), (9, 20), (7, 15), (8, 17), (6, 13)] {
            for (w, h) in [(40u16, 22u16), (60, 28), (40, 10), (24, 38), (30, 6)] {
                let Some((cols, rows)) = art_frame_cells(Rect::new(0, 0, w, h), cell_w, cell_h)
                else {
                    continue;
                };
                let (px_w, px_h) = (cols as u32 * cell_w as u32, rows as u32 * cell_h as u32);
                assert!(
                    px_w <= px_h && px_h - px_w < cell_w as u32,
                    "{w}x{h} at {cell_w}x{cell_h}: frame is {px_w}x{px_h} px ({cols}x{rows} cells)"
                );
            }
        }
    }
}
