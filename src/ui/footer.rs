// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The one-line context hint along the bottom.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::*;
use crate::app::App;

pub(super) fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Length(20)]).split(area);

    let context_hint = get_context_hint(app);
    let context_span = match app.update.available.as_deref() {
        // Done means the binary on disk is already the new one; the hint has
        // to switch from "install this" to "you are still running the old one".
        Some(tag) if app.update.status == crate::app::UpdateStatus::Done => Span::styled(
            format!("✓ {tag} — restart  {context_hint}"),
            Style::default().fg(DIM),
        ),
        Some(tag) => Span::styled(
            format!("↑ {tag} — U  {context_hint}"),
            Style::default().fg(DIM),
        ),
        None => Span::styled(context_hint, Style::default().fg(DIM)),
    };
    f.render_widget(
        Paragraph::new(Line::from(context_span)).alignment(Alignment::Left),
        cols[0],
    );

    let help_span = Span::styled(
        "h/? show keybinds",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(
        Paragraph::new(Line::from(help_span)).alignment(Alignment::Right),
        cols[1],
    );
}

pub(super) fn get_context_hint(app: &App) -> String {
    if let Some(View::ArtistDetail(detail)) = app.view_stack.last() {
        match detail.focus {
            ArtistDetailFocus::Tracks => {
                "↑↓ Select | ← → Section | a Add | f Fav | r Radio".to_string()
            }
            ArtistDetailFocus::Albums | ArtistDetailFocus::EPs | ArtistDetailFocus::Singles => {
                "↑↓ Select | ← → Section | f Fav | c Copy".to_string()
            }
            ArtistDetailFocus::Bio => "↑↓ Scroll | ← → Section".to_string(),
        }
    } else if let Some(View::AlbumDetail(_)) = app.view_stack.last() {
        "↑↓ Select | a Add | f Fav | r Radio | c Copy".to_string()
    } else if let Some(View::PlaylistDetail(detail)) = app.view_stack.last() {
        match detail.focus {
            PlaylistDetailFocus::Tracks => {
                "↑↓ Select | ← → Section | a Add | f Fav | r Radio | c Copy".to_string()
            }
            PlaylistDetailFocus::Description => "↑↓ Scroll | ← → Section".to_string(),
        }
    } else {
        match app.current_tab {
            Tab::Home => "↑↓ Select | ← → Switch section | Enter Open".to_string(),
            Tab::Favorites => "↑↓ Select | a Add | f Fav | r Radio | c Copy".to_string(),
            Tab::Artists => "↑↓ Select | f Follow | Enter Open".to_string(),
            Tab::Albums => "↑↓ Select | f Fav | Enter Open".to_string(),
            Tab::Playlists => "↑↓ Select | Enter Open".to_string(),
            Tab::Search => "↑↓ Select | ← → Section | / Open Search".to_string(),
        }
    }
}
