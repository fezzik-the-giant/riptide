// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The queue panel down the right-hand side.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
};

use super::*;
use crate::app::App;

pub(super) fn render_queue(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.queue_focused;
    // Both palette colours, so the divider tracks the terminal theme. A fixed
    // dark grey read *stronger* than the focused accent against a light one,
    // which inverted the signal it was there to give.
    let border_style = Style::default().fg(if focused { ACCENT } else { DIM });
    // No title on the block — ratatui doesn't reserve a row for titles on
    // Borders::LEFT-only blocks, so the title would be overdrawn by content.
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Title row rendered manually at the top of the inner area.
    let queue_title = if app.now_playing.shuffle {
        " Queue ⇄ "
    } else {
        " Queue "
    };
    let title_style = if focused {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    f.render_widget(
        Paragraph::new(Span::styled(queue_title, title_style)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Content area starts one row below the title.
    let content_y = inner.y + 1;
    let content_h = inner.height.saturating_sub(1);

    let queue = &app.now_playing.queue;
    if queue.is_empty() {
        if content_h > 0 {
            f.render_widget(
                Paragraph::new("no queue")
                    .style(Style::default().fg(DIM))
                    .alignment(Alignment::Center),
                Rect::new(inner.x, content_y, inner.width, content_h),
            );
        }
        return;
    }

    let current = app.now_playing.queue_index;
    let cursor = app.queue_cursor;
    let item_h = 2usize;
    let visible = (content_h as usize).saturating_div(item_h).max(1);
    let offset = app.queue_scroll_offset(visible);

    let mut y = content_y;
    for (i, track) in queue.iter().enumerate().skip(offset) {
        if y + 1 >= content_y + content_h {
            break;
        }
        let is_cur = i == current;
        let is_cursor = focused && i == cursor && !app.help_active;
        let heart = if app.favorite_track_ids.contains(&track.id) {
            " ❤"
        } else {
            ""
        };
        // The cursor takes the same filled bar as the library lists, so the row
        // the keys act on looks identical whichever pane holds focus — the
        // divider alone was a single column and easy to miss. The now-playing
        // marker keeps its own colour and shows through underneath.
        let prefix = if is_cur {
            "♪ "
        } else if is_cursor {
            "▶ "
        } else {
            ""
        };
        let title_line = format!("{prefix}{}{}", track.title, heart);
        let line_style = if is_cursor {
            row_style(true)
        } else if is_cur {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        f.render_widget(
            Paragraph::new(ellipsize(&title_line, inner.width)).style(line_style),
            Rect::new(inner.x, y, inner.width, 1),
        );
        f.render_widget(
            Paragraph::new(ellipsize(
                &format!("  {}", track.all_artist_names()),
                inner.width,
            ))
            .style(line_style),
            Rect::new(inner.x, y + 1, inner.width, 1),
        );
        y += item_h as u16;
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Tab;
    use crate::app::test_support::{test_app, track};
    use ratatui::{Terminal, backend::TestBackend};

    /// Rows carrying a filled cursor bar within `xs`. The bar is the only thing
    /// that paints a background, so this finds the cursor in one pane without
    /// caring what the row says. The panes sit side by side, so a row index
    /// alone would not tell them apart.
    fn bar_rows(app: &crate::app::App, w: u16, h: u16, xs: std::ops::Range<u16>) -> Vec<u16> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..h)
            .filter(|&y| {
                xs.clone()
                    .any(|x| buf.cell((x, y)).unwrap().bg == super::HIGHLIGHT_BG)
            })
            .collect()
    }

    /// Focus used to be announced by one column of border colour, so both panes
    /// looked equally live. Whichever pane holds it now draws the cursor bar and
    /// the other gives it up — two bars mean neither reads as the key target.
    #[test]
    fn the_cursor_bar_moves_to_the_queue_and_the_library_gives_it_up() {
        let mut t = test_app();
        t.app.current_tab = Tab::Favorites;
        t.app.favorites.append_page(vec![track(1), track(2)], None);
        t.app.now_playing.queue = vec![track(1), track(2)];
        t.app.queue_visible = true;
        std::mem::forget(t.api_rx);

        let (w, h) = (100u16, 24u16);
        let library = 0..w - super::QUEUE_W;
        let queue = w - super::QUEUE_W..w;

        t.app.queue_focused = false;
        assert!(
            !bar_rows(&t.app, w, h, library.clone()).is_empty(),
            "library lost its cursor while it had focus"
        );
        assert!(
            bar_rows(&t.app, w, h, queue.clone()).is_empty(),
            "queue drew a cursor without focus"
        );

        t.app.queue_focused = true;
        assert!(
            !bar_rows(&t.app, w, h, queue).is_empty(),
            "queue drew no cursor while it had focus"
        );
        assert!(
            bar_rows(&t.app, w, h, library).is_empty(),
            "library kept its cursor after focus moved to the queue"
        );
    }
}
