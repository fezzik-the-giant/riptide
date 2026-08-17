// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! The Home tab: new releases, daily mixes and discovery mixes.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::*;
use crate::app::App;

pub(super) fn render_home(f: &mut Frame, app: &App, area: Rect) {
    use crate::app::HomeSectionFocus;

    let any_loading = app.home_new_releases.loading
        || app.home_daily_mixes.loading
        || app.home_discovery_mixes.loading;

    let title = if any_loading {
        let spinner = spinner_char(app.tick);
        format!(" Loading {} ", spinner)
    } else {
        " Home ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(ACCENT));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Only show sections once all are loaded
    if any_loading {
        let loading_text = "Fetching mixes...";
        let paragraph = Paragraph::new(loading_text)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center);
        f.render_widget(paragraph, inner);
        return;
    }

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(inner);

    render_home_section(
        f,
        &app.home_new_releases,
        chunks[0],
        "New Releases",
        app.home_section_focus == HomeSectionFocus::NewReleases,
    );
    render_home_section(
        f,
        &app.home_daily_mixes,
        chunks[1],
        "Daily Mixes",
        app.home_section_focus == HomeSectionFocus::DailyMixes,
    );
    render_home_section(
        f,
        &app.home_discovery_mixes,
        chunks[2],
        "Daily Discovery",
        app.home_section_focus == HomeSectionFocus::DiscoveryMixes,
    );
}

pub(super) fn render_home_section(
    f: &mut Frame,
    section: &crate::app::HomeSection<crate::api::models::Playlist>,
    area: Rect,
    title: &str,
    focused: bool,
) {
    let border_color = if focused { ACCENT } else { Color::Gray };

    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if section.loading {
        let spinner = spinner_char(0);
        let text = format!("{} Loading...", spinner);
        let paragraph = Paragraph::new(text).style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, inner);
        return;
    }

    if let Some(ref error) = section.error {
        let paragraph =
            Paragraph::new(format!("Error: {}", error)).style(Style::default().fg(Color::Red));
        f.render_widget(paragraph, inner);
        return;
    }

    if section.items.is_empty() {
        let paragraph = Paragraph::new("No items").style(Style::default().fg(Color::Gray));
        f.render_widget(paragraph, inner);
        return;
    }

    let height = inner.height as usize;
    let start = if section.selected < height {
        0
    } else {
        section.selected - height + 1
    };

    let visible_items: Vec<ListItem> = section.items[start..]
        .iter()
        .take(height)
        .enumerate()
        .map(|(i, item)| {
            let idx = start + i;
            let is_selected = idx == section.selected && focused;

            let title_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else if focused {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let line = Line::from(Span::styled(
                format!("{}{}", prefix, item.title),
                title_style,
            ));
            ListItem::new(line)
        })
        .collect();

    let list = List::new(visible_items);
    f.render_widget(list, inner);
}
