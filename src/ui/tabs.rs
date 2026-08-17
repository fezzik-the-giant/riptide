// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! The tab strip across the top of the window.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph},
};

use super::*;
use crate::app::App;

/// Horizontal tab strip across the top, with a box drawn around the active tab.
/// Colours match `render_carousel_tabs` in the artist and search views so
/// navigation reads the same everywhere.
///
/// Tabs are placed one at a time rather than as a single centered `Line` so the
/// active tab's exact column span is known and the box can be drawn around it.
pub(super) fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    const SEP: &str = " - ";

    if area.height < 3 || area.width == 0 {
        return;
    }

    let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
    let sep_w = SEP.chars().count() as u16;
    let total_w: u16 = titles.iter().map(|t| t.chars().count() as u16).sum::<u16>()
        + sep_w * titles.len().saturating_sub(1) as u16;

    // Centre the strip, leaving room for the active tab's left border.
    let mut x = area.x + area.width.saturating_sub(total_w) / 2;
    let text_y = area.y + 1;

    let mut active: Option<(u16, u16)> = None;

    for (i, tab) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            f.render_widget(
                Paragraph::new(SEP).style(Style::default().fg(DIM)),
                Rect::new(x, text_y, sep_w, 1),
            );
            x += sep_w;
        }

        let w = titles[i].chars().count() as u16;
        let selected = app.current_tab == *tab;
        if selected {
            active = Some((x, w));
        }

        let style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        f.render_widget(
            Paragraph::new(titles[i]).style(style),
            Rect::new(x, text_y, w, 1),
        );

        x += w;
    }

    // Drawn after the loop: the box hugs the label with one cell either side,
    // overwriting the padding spaces of the adjacent " - " separators. Doing it
    // inside the loop would let the *next* tab's separator erase the right
    // border. A bare Block only paints border cells, so the label survives.
    if let Some((tab_x, tab_w)) = active {
        let box_x = tab_x.saturating_sub(1);
        let box_w = (tab_w + 2).min(area.right().saturating_sub(box_x));
        f.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT)),
            Rect::new(box_x, area.y, box_w, 3),
        );
    }
}
