// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Column layout for list rows.
//!
//! Rows used to be one concatenated string, which the terminal clipped at the
//! right edge — so a long title pushed the duration, quality badge and favourite
//! marker off screen, losing the metadata to keep the least useful characters.
//! Laying a row out in columns pins that metadata to the right and ellipsizes
//! only the text that can afford it.

use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// How a cell claims its share of the row.
#[derive(Clone, Copy)]
pub(super) enum Width {
    /// Exactly this many columns, always drawn. For short metadata whose whole
    /// point is to stay visible.
    Fixed(u16),
    /// Shares what the fixed columns leave over, split by `weight` and
    /// ellipsized to fit. Dropped entirely when its share would fall below
    /// `min`, which is how the artist column gets out of the way on a narrow
    /// terminal. A `min` of 0 means "squeeze, never drop".
    Flex { weight: u16, min: u16 },
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Align {
    Left,
    Right,
}

pub(super) struct Cell {
    pub text: String,
    pub style: Style,
    pub width: Width,
    pub align: Align,
}

impl Cell {
    pub fn fixed(text: impl Into<String>, columns: u16, style: Style) -> Self {
        Cell {
            text: text.into(),
            style,
            width: Width::Fixed(columns),
            align: Align::Left,
        }
    }

    pub fn flex(text: impl Into<String>, weight: u16, min: u16, style: Style) -> Self {
        Cell {
            text: text.into(),
            style,
            width: Width::Flex { weight, min },
            align: Align::Left,
        }
    }

    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }
}

/// How long the row sits still before it starts moving, and pauses again once it
/// reaches the far end.
const MARQUEE_HOLD: Duration = Duration::from_millis(700);

/// How long each column of travel takes.
const MARQUEE_STEP: Duration = Duration::from_millis(90);

/// How far into an overflowing string the marquee has travelled.
///
/// Ping-pongs rather than wrapping: scrolling back the way it came needs no
/// separator to explain itself, and never shows a title spliced to its own tail.
fn marquee_offset(overflow: u64, elapsed: Duration) -> usize {
    let hold = MARQUEE_HOLD.as_millis() as u64;
    let step = MARQUEE_STEP.as_millis() as u64;
    let leg = hold + overflow * step;
    let travelled = match elapsed.as_millis() as u64 % (2 * leg) {
        t if t < hold => 0,
        t if t < leg => (t - hold) / step,
        t if t < leg + hold => overflow,
        t => overflow - (t - leg - hold) / step,
    };
    travelled.min(overflow) as usize
}

/// The slice of `text` visible this frame, `columns` wide.
///
/// A wide character straddling the left edge is drawn as a single space, so the
/// window stays exactly `columns` wide instead of jittering by one as it passes.
pub(super) fn marquee(text: &str, columns: u16, elapsed: Duration) -> String {
    let columns = columns as usize;
    if columns == 0 || text.width() <= columns {
        return text.to_string();
    }
    let offset = marquee_offset((text.width() - columns) as u64, elapsed);

    let mut out = String::new();
    let mut x = 0usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if x + w <= offset {
            x += w;
            continue;
        }
        if x < offset {
            // Straddles the left edge: show the half that fits.
            out.push(' ');
            x += w;
            continue;
        }
        if x + w > offset + columns {
            break;
        }
        out.push(ch);
        x += w;
    }
    out
}

/// Lay `cells` out across `total` columns.
///
/// Flex cells keep a one-column gap after themselves, so callers only pad fixed
/// cells. Anything that does not fit is dropped rather than wrapped: a list row
/// is one line.
pub(super) fn layout_row(total: u16, cells: Vec<Cell>, phase: Option<Duration>) -> Line<'static> {
    let fixed: u16 = cells
        .iter()
        .filter_map(|c| match c.width {
            Width::Fixed(w) => Some(w),
            Width::Flex { .. } => None,
        })
        .sum();

    let mut spare = total.saturating_sub(fixed);

    // Drop flex columns right to left until the rest clear their minimum, so the
    // artist gives way before the title does.
    let mut keep: Vec<bool> = cells.iter().map(|_| true).collect();
    loop {
        let weights: u16 = cells
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .filter_map(|(c, _)| match c.width {
                Width::Flex { weight, .. } => Some(weight),
                Width::Fixed(_) => None,
            })
            .sum();
        if weights == 0 {
            break;
        }
        let starved = cells
            .iter()
            .enumerate()
            .zip(&keep)
            .rev()
            .find_map(|((i, c), k)| match (*k, c.width) {
                (true, Width::Flex { weight, min })
                    if min > 0 && spare * weight / weights < min =>
                {
                    Some(i)
                }
                _ => None,
            });
        match starved {
            Some(i) => keep[i] = false,
            None => break,
        }
    }

    let weights: u16 = cells
        .iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .filter_map(|(c, _)| match c.width {
            Width::Flex { weight, .. } => Some(weight),
            Width::Fixed(_) => None,
        })
        .sum();

    let mut spans = Vec::with_capacity(cells.len());
    let mut remaining_weight = weights;
    for (cell, kept) in cells.iter().zip(&keep) {
        if !kept {
            continue;
        }
        let columns = match cell.width {
            Width::Fixed(w) => w,
            Width::Flex { weight, .. } => {
                // Hand the last flex column whatever rounding left behind.
                let share = if weight == remaining_weight {
                    spare
                } else {
                    spare * weight / remaining_weight.max(1)
                };
                remaining_weight = remaining_weight.saturating_sub(weight);
                spare = spare.saturating_sub(share);
                share
            }
        };
        if columns == 0 {
            continue;
        }
        // The gap belongs to the flex column, so callers pad only fixed ones.
        let text_columns = match cell.width {
            Width::Flex { .. } => columns.saturating_sub(1),
            Width::Fixed(_) => columns,
        };
        // Only the row under the cursor scrolls; the rest stay still and
        // ellipsized, or the whole list would be in motion at once.
        let shown = match phase {
            Some(phase) => marquee(&cell.text, text_columns, phase),
            None => ellipsize(&cell.text, text_columns),
        };
        spans.push(Span::styled(pad(&shown, columns, cell.align), cell.style));
    }

    Line::from(spans)
}

/// Cut `text` to `columns` display columns, marking the cut with an ellipsis.
///
/// Measured in display columns rather than chars: a library holds CJK titles and
/// emoji (a playlist literally named 🔥), and counting chars would misalign every
/// column on those rows.
pub(super) fn ellipsize(text: &str, columns: u16) -> String {
    let columns = columns as usize;
    if columns == 0 {
        return String::new();
    }
    if text.width() <= columns {
        return text.to_string();
    }
    if columns == 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > columns - 1 {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn pad(text: &str, columns: u16, align: Align) -> String {
    let fill = (columns as usize).saturating_sub(text.width());
    match align {
        Align::Left => format!("{text}{}", " ".repeat(fill)),
        Align::Right => format!("{}{text}", " ".repeat(fill)),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn ellipsize_counts_display_columns_not_chars() {
        // Four CJK chars are eight columns wide, so six columns fits two of them
        // plus the ellipsis. Counting chars would have fitted five and overflowed.
        assert_eq!(ellipsize("中文樂隊", 6), "中文…");
        assert_eq!(ellipsize("中文樂隊", 8), "中文樂隊");
        assert_eq!(ellipsize("🔥🔥🔥", 4), "🔥…");
        assert_eq!(ellipsize("abcdef", 4), "abc…");
        assert_eq!(ellipsize("abc", 9), "abc");
        assert_eq!(ellipsize("abc", 1), "…");
        assert_eq!(ellipsize("abc", 0), "");
    }

    /// Whatever the text does, the row is exactly as wide as it was given — that
    /// is what keeps the metadata columns lined up between rows. It has to hold
    /// mid-scroll too, or the selected row would shove the columns around.
    #[test]
    fn a_row_always_fills_its_width() {
        let style = Style::default();
        for width in [20u16, 40, 74, 120] {
            for phase in [
                None,
                Some(Duration::ZERO),
                Some(Duration::from_millis(900)),
                Some(Duration::from_millis(2000)),
                Some(Duration::from_millis(4000)),
            ] {
                let line = layout_row(
                    width,
                    vec![
                        Cell::fixed("▶ ", 2, style),
                        Cell::flex("Kingslayer (feat. BABYMETAL)", 3, 0, style),
                        Cell::flex("中文樂隊中文樂隊中文樂隊", 2, 12, style),
                        Cell::fixed("3:58", 6, style).right(),
                    ],
                    phase,
                );
                assert_eq!(
                    rendered(&line).width(),
                    width as usize,
                    "width {width}, phase {phase:?}"
                );
            }
        }
    }

    /// Holds at the start long enough to read it, walks to the end, holds again,
    /// then comes back the way it went.
    #[test]
    fn the_marquee_holds_at_both_ends_and_returns() {
        let text = "Kingslayer (feat. BABYMETAL) - Extended Mix";
        let window = |phase| marquee(text, 20, phase);

        let ms = Duration::from_millis;

        let start = window(ms(0));
        assert!(text.starts_with(&start), "{start}");
        assert_eq!(
            window(MARQUEE_HOLD - ms(1)),
            start,
            "still held at the start"
        );
        assert_ne!(window(MARQUEE_HOLD + MARQUEE_STEP), start, "then it moves");

        let overflow = (text.width() - 20) as u32;
        let leg = MARQUEE_HOLD + MARQUEE_STEP * overflow;
        let end = window(leg);
        assert!(text.ends_with(&end), "{end}");
        assert_eq!(
            window(leg + MARQUEE_HOLD - ms(1)),
            end,
            "held at the end too"
        );

        // A full cycle later it is back where it began.
        assert_eq!(window(2 * leg), start);
    }

    #[test]
    fn text_that_fits_never_moves() {
        for ms in [0u64, 500, 3000, 30_000] {
            assert_eq!(
                marquee("Lanterns", 20, Duration::from_millis(ms)),
                "Lanterns"
            );
        }
    }

    /// The artist column gives way before the title does, and hands over its
    /// space rather than leaving a hole.
    #[test]
    fn a_starved_flex_column_is_dropped_rightmost_first() {
        let style = Style::default();
        let row = |width| {
            let line = layout_row(
                width,
                vec![
                    Cell::flex("TITLE-TITLE-TITLE", 3, 0, style),
                    Cell::flex("ARTIST", 2, 12, style),
                    Cell::fixed("3:58", 6, style).right(),
                ],
                None,
            );
            rendered(&line)
        };

        let wide = row(60);
        assert!(wide.contains("ARTIST"), "{wide}");
        assert!(wide.contains("TITLE-TITLE-TITLE"), "{wide}");

        let narrow = row(30);
        assert!(!narrow.contains("ARTIST"), "{narrow}");
        assert!(narrow.contains("TITLE-TITLE-TITLE"), "{narrow}");
        assert!(narrow.contains("3:58"), "{narrow}");
    }
}
