// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The tab strip shared by the artist, search and home views.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

use super::*;

const SEPARATOR: &str = " - ";

/// Width the strip needs before the title starts truncating, borders and the
/// padding `render_carousel` adds included.
pub(super) fn carousel_width(labels: &[(String, bool)]) -> u16 {
    let text: usize = labels.iter().map(|(l, _)| l.chars().count()).sum();
    let separators = labels.len().saturating_sub(1) * SEPARATOR.chars().count();
    (text + separators) as u16 + 4
}

/// Draw the bordered block whose title is the tab strip, and return the area to
/// render the active tab's content into. `None` when there is no room for any.
pub(super) fn render_carousel(
    f: &mut Frame,
    area: Rect,
    labels: &[(String, bool)],
) -> Option<Rect> {
    if area.height < 2 {
        return None;
    }

    let mut spans = vec![Span::raw(" ")];
    for (i, (label, active)) in labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(SEPARATOR, Style::default().fg(DIM)));
        }
        let style = if *active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        spans.push(Span::styled(label.clone(), style));
    }
    spans.push(Span::raw(" "));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Line::from(spans));

    let inner = block.inner(area);
    f.render_widget(block, area);

    (inner.height > 0).then_some(inner)
}
