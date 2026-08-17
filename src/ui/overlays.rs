// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Modal overlays: command palette, sort, artist picker, help, toasts.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::*;
use crate::app::App;

pub(super) fn render_command_overlay(f: &mut Frame, app: &App, area: Rect) {
    let matches = app.command.matches();

    let box_w: u16 = 34;
    // border(2) + input(1) + divider(1) + items (at least 1)
    let box_h: u16 = 4 + matches.len().max(1) as u16;

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(
            " command ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    if inner.height == 0 {
        return;
    }

    // Input line: "/ <typed><ghost>█"
    let q_lower = app.command.input.to_lowercase();
    let ghost = matches.first().map(|m| &m[q_lower.len()..]).unwrap_or("");
    let cursor = cursor_char(app.tick);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "/ ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(app.command.input.clone(), Style::default().fg(Color::White)),
            Span::styled(ghost, Style::default().fg(DIM)),
            Span::styled(cursor, Style::default().fg(Color::White)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height < 2 {
        return;
    }

    // Thin divider between input and list
    f.render_widget(
        Paragraph::new("─".repeat(inner.width as usize)).style(Style::default().fg(DIM)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );

    // Command rows
    if matches.is_empty() {
        f.render_widget(
            Paragraph::new(" no match").style(Style::default().fg(DIM)),
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
        );
    } else {
        for (i, cmd) in matches.iter().enumerate() {
            let row_y = inner.y + 2 + i as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let selected = i == app.command.selected;
            let style = if selected {
                Style::default()
                    .bg(SELECT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            f.render_widget(
                Paragraph::new(format!(" {cmd}")).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }
}

pub(super) fn render_sort_overlay(f: &mut Frame, app: &App, area: Rect) {
    use crate::app::SortPalette;

    let options = SortPalette::get_options(app.current_tab);
    let box_w: u16 = 26;
    let box_h: u16 = 2 + options.len() as u16; // border top/bottom + one row per option

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(
            " sort by ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    for (i, (label, _)) in options.iter().enumerate() {
        let row_y = inner.y + i as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let selected = i == app.sort_palette.selected;
        let style = if selected {
            Style::default()
                .bg(SELECT_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        let prefix = if selected { " ► " } else { "   " };
        f.render_widget(
            Paragraph::new(format!("{prefix}{label}")).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }
}

pub(super) fn render_artist_selection_modal(f: &mut Frame, app: &App, area: Rect) {
    let box_w = 40u16.min(area.width.saturating_sub(4));
    let box_h =
        (4 + app.artist_selection.artist_names.len() as u16).min(area.height.saturating_sub(6));

    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let overlay = Rect::new(
        x.min(area.right().saturating_sub(box_w)),
        y.min(area.bottom().saturating_sub(box_h)),
        box_w.min(area.width),
        box_h.min(area.height),
    );

    f.render_widget(Clear, overlay);
    let block = Block::default()
        .title(Span::styled(
            " select artist ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);
    f.render_widget(block, overlay);

    let start_y = inner.y;
    for (i, name) in app.artist_selection.artist_names.iter().enumerate() {
        let row_y = start_y + i as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let selected = i == app.artist_selection.selected;
        let style = if selected {
            Style::default()
                .bg(SELECT_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if selected { " ► " } else { "   " };
        let line = format!("{prefix}{}", name);
        f.render_widget(
            Paragraph::new(line).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }
}

pub(super) fn render_help_modal(f: &mut Frame, app: &App, area: Rect) {
    // Fixed size modal, well clear of the now-playing bar (9 lines at bottom)
    let box_w = 50u16.min(area.width.saturating_sub(4));
    let box_h = 24u16.min(area.height.saturating_sub(12)); // Leave 9 for now-playing + margin

    // Center horizontally and vertically within the safe area
    let safe_bottom = area.bottom().saturating_sub(10);
    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + (safe_bottom - area.y).saturating_sub(box_h) / 2;

    let overlay = Rect::new(x, y, box_w, box_h);

    let block = Block::default()
        .title(Span::styled(
            " help ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(overlay);

    // Clear only the inner content area
    f.render_widget(Clear, inner);
    f.render_widget(block, overlay);

    if inner.height == 0 || inner.width < 20 {
        return;
    }

    // Collect all keybind groups
    let groups = vec![
        KeybindGroup::global(),
        KeybindGroup::navigation(),
        KeybindGroup::queue(),
        KeybindGroup::search(),
        KeybindGroup::command(),
    ];

    // Build lines for rendering
    let mut lines: Vec<Line> = Vec::new();
    for group in groups {
        // Group header
        lines.push(Line::from(Span::styled(
            group.title,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));

        // Keybinds
        for keybind in group.binds {
            let line = Line::from(vec![
                Span::styled(
                    format!("  {:<12}", keybind.key),
                    Style::default().fg(ACCENT),
                ),
                Span::raw(keybind.action),
            ]);
            lines.push(line);
        }

        // Space between groups
        lines.push(Line::from(""));
    }

    // Render with scrolling
    let start = app.help_scroll as usize;
    let mut y = inner.y;

    for line in lines.iter().skip(start) {
        if y >= inner.y + inner.height {
            break;
        }
        f.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    // Render scroll hint
    if lines.len() as u16 > app.help_scroll + inner.height {
        let hint = " ↓ more ";
        f.render_widget(
            Paragraph::new(hint)
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Right),
            Rect::new(
                inner.x,
                inner.y + inner.height.saturating_sub(1),
                inner.width,
                1,
            ),
        );
    }
}

pub(super) fn render_toast(f: &mut Frame, app: &App, area: Rect) {
    let Some((msg, level, set_at)) = &app.status else {
        return;
    };

    let elapsed = set_at.elapsed().as_secs_f64();
    // Fade out over the last ~1 s of the 5 s lifetime.
    let fading = elapsed > 4.0;

    let (border_color, text_color) = match level {
        StatusLevel::Error => (
            Color::Red,
            if fading {
                Color::DarkGray
            } else {
                Color::White
            },
        ),
        StatusLevel::Info => (
            ACCENT,
            if fading {
                Color::DarkGray
            } else {
                Color::White
            },
        ),
    };

    // Size the card to the message, clamped to the terminal width.
    let inner_w = msg.len() as u16 + 4; // 2 padding each side
    let toast_w = inner_w.min(area.width.saturating_sub(4));
    let toast_h = 3u16;
    let x = area.x + area.width.saturating_sub(toast_w) / 2;
    // Float just above the now-playing bar (last 10 rows).
    let y = area.y + area.height.saturating_sub(toast_h + 10);
    let toast_rect = Rect::new(x, y, toast_w, toast_h);

    f.render_widget(Clear, toast_rect);
    f.render_widget(
        Paragraph::new(msg.as_str())
            .alignment(Alignment::Center)
            .style(Style::default().fg(text_color))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color)),
            ),
        toast_rect,
    );
}
