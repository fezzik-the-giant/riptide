// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Search: the query box and the three result panes.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::*;
use crate::app::App;

pub(super) fn render_search_modal(f: &mut Frame, app: &App, area: Rect) {
    // Center a search input modal
    let modal_width = 60.min(area.width - 4);
    let modal_height = 3;

    let h_centered = Layout::horizontal([
        Constraint::Length((area.width.saturating_sub(modal_width)) / 2),
        Constraint::Length(modal_width),
        Constraint::Min(0),
    ])
    .split(area);

    let v_centered = Layout::vertical([
        Constraint::Length((area.height.saturating_sub(modal_height)) / 2),
        Constraint::Length(modal_height),
        Constraint::Min(0),
    ])
    .split(h_centered[1]);

    let modal_area = v_centered[1];

    // Render modal background/border
    let block = Block::default()
        .title(" Search ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(modal_area);
    f.render_widget(block, modal_area);

    // Render search input
    let cursor = cursor_char(app.tick);
    let input_text = format!("{}{}", app.search.query, cursor);
    f.render_widget(
        Paragraph::new(input_text)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Left),
        inner,
    );
}

pub(super) fn render_search_results(f: &mut Frame, app: &App, area: Rect) {
    // Show modal if open
    if app.search.modal_open {
        render_search_modal(f, app, area);
        return;
    }

    // Empty state — no results and not loading
    if app.search.total_results() == 0 && !app.search.loading {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(ACCENT));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let rows = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(inner);

        // The modal is closed here, so keystrokes do not reach a query box —
        // point at the key that reopens it rather than saying "start typing".
        let content: Line = if app.search.query.is_empty() {
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(DIM)),
                Span::styled(
                    "/",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to search", Style::default().fg(DIM)),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    format!("No results for \"{}\" — press ", app.search.query),
                    Style::default().fg(DIM),
                ),
                Span::styled(
                    "/",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to search again", Style::default().fg(DIM)),
            ])
        };
        f.render_widget(
            Paragraph::new(content).alignment(Alignment::Center),
            rows[1],
        );
        return;
    }

    // Loading state
    if app.search.loading {
        let spinner = spinner_char(app.tick);
        let block = Block::default()
            .title(format!(" Searching {spinner} "))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(DIM));
        f.render_widget(block, area);
        return;
    }

    render_search_carousel_tabs(f, app, area);
}

pub(super) fn render_search_carousel_tabs(f: &mut Frame, app: &App, area: Rect) {
    if area.height < 2 {
        return;
    }

    let tabs = vec![
        (
            format!(" Tracks ({}) ", app.search.tracks.len()),
            SearchPane::Tracks,
        ),
        (
            format!("Artists ({})", app.search.artists.len()),
            SearchPane::Artists,
        ),
        (
            format!("Playlists ({}) ", app.search.playlists.len()),
            SearchPane::Playlists,
        ),
    ];

    let mut line_spans = Vec::new();
    for (i, (name, pane)) in tabs.iter().enumerate() {
        if i > 0 {
            line_spans.push(Span::styled(" - ", Style::default().fg(DIM)));
        }

        let selected = app.search.pane == *pane;
        let style = if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };

        line_spans.push(Span::styled(name.clone(), style));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Line::from(line_spans));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height > 0 {
        match app.search.pane {
            SearchPane::Tracks => render_search_pane_tracks(f, app, inner),
            SearchPane::Artists => render_search_pane_artists(f, app, inner),
            SearchPane::Playlists => render_search_pane_playlists(f, app, inner),
        }
    }
}

pub(super) fn render_search_pane_tracks(f: &mut Frame, app: &App, area: Rect) {
    let sel = app.search.track_sel;
    let height = area.height as usize;
    let offset = app.search.track_scroll_offset(height);
    let items: Vec<ListItem> = app
        .search
        .tracks
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, t)| {
            let selected = i == sel;
            let is_playing = app
                .now_playing
                .track
                .as_ref()
                .map(|np| np.id == t.id)
                .unwrap_or(false);
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let playing = if is_playing { "♪ " } else { "" };

            let title_span = Span::styled(
                format!(
                    "{prefix}{playing}{} — {} ({})",
                    t.title,
                    t.all_artist_names(),
                    t.duration_display()
                ),
                style,
            );

            let badge = t
                .quality_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            let badge_span = Span::styled(
                badge,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            );

            let heart = if app.favorite_track_ids.contains(&t.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![title_span, badge_span, heart]))
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No tracks")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            area,
        );
    } else {
        f.render_widget(List::new(items), area);
    }
}

pub(super) fn render_search_pane_artists(f: &mut Frame, app: &App, area: Rect) {
    let sel = app.search.artist_sel;
    let height = area.height as usize;
    let offset = app.search.artist_scroll_offset(height);
    let items: Vec<ListItem> = app
        .search
        .artists
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, a)| {
            let selected = i == sel;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            let title_span = Span::styled(format!("{prefix}{}", a.name), style);
            let heart = if app.favorite_artist_ids.contains(&a.id) {
                Span::raw(" ❤")
            } else {
                Span::raw("")
            };
            ListItem::new(Line::from(vec![title_span, heart]))
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No artists")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            area,
        );
    } else {
        f.render_widget(List::new(items), area);
    }
}

pub(super) fn render_search_pane_playlists(f: &mut Frame, app: &App, area: Rect) {
    let sel = app.search.playlist_sel;
    let height = area.height as usize;
    let offset = app.search.playlist_scroll_offset(height);
    let items: Vec<ListItem> = app
        .search
        .playlists
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, pl)| {
            let selected = i == sel;
            let style = if selected {
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if selected { "▶ " } else { "  " };
            ListItem::new(format!(
                "{prefix}{} ({} tracks)",
                pl.title,
                pl.number_of_tracks.unwrap_or(0)
            ))
            .style(style)
        })
        .collect();

    if items.is_empty() {
        f.render_widget(
            Paragraph::new("No playlists")
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            area,
        );
    } else {
        f.render_widget(List::new(items), area);
    }
}
