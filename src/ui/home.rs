// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The Home tab: new releases, daily mixes and discovery mixes.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{List, ListItem, Paragraph, Wrap},
};

use super::*;
use crate::app::{App, HomeSectionFocus};

/// Narrowest art column worth drawing into. Below this the cover is a smudge
/// and the mixes are better off with the width.
const HOME_ART_MIN_W: u16 = 12;

/// Blank columns between the art's frame and the mixes beside it.
const HOME_ART_GAP: u16 = 2;

pub(super) fn render_home(f: &mut Frame, app: &App, area: Rect) {
    let labels = [
        section_label(app, HomeSectionFocus::NewReleases),
        section_label(app, HomeSectionFocus::DailyMixes),
        section_label(app, HomeSectionFocus::DiscoveryMixes),
    ];

    // The art only gets a column once the tab strip has the room it needs;
    // otherwise the section names truncate away and navigation loses its map.
    let mut bounds = sidebar_art_bounds(area);
    bounds.width = bounds
        .width
        .min(area.width.saturating_sub(carousel_width(&labels)));
    let art_w = bounds.width;
    let frame = art_frame(bounds);
    // The column is cut to the frame rather than the frame floated in the
    // column, so whatever the art cannot use on a short window goes to the mixes.
    let list_area = if art_w >= HOME_ART_MIN_W && !frame.is_empty() {
        let art_col_w = frame.width + HOME_ART_GAP;
        let split =
            Layout::horizontal([Constraint::Length(art_col_w), Constraint::Min(0)]).split(area);
        render_home_art(f, app, split[0], frame);
        split[1]
    } else {
        area
    };

    let Some(inner) = render_carousel(f, list_area, &labels) else {
        return;
    };

    render_home_section(f, app, app.home_section(), inner);
}

/// A section's tab label: its own spinner while loading, its count once it has
/// arrived. Each section is fetched separately, so one still in flight no longer
/// holds up the two that are ready.
fn section_label(app: &App, section: HomeSectionFocus) -> (String, bool) {
    let (name, state) = match section {
        HomeSectionFocus::NewReleases => ("New Releases", &app.home_new_releases),
        HomeSectionFocus::DailyMixes => ("Daily Mixes", &app.home_daily_mixes),
        HomeSectionFocus::DiscoveryMixes => ("Daily Discovery", &app.home_discovery_mixes),
    };
    let label = if state.loading {
        format!("{name} {}", spinner_char(app.tick))
    } else {
        format!("{name} ({})", state.items.len())
    };
    (label, app.home_section_focus == section)
}

fn render_home_art(f: &mut Frame, app: &App, area: Rect, frame: Rect) {
    render_framed_art(
        f,
        frame,
        app.home_art.bytes.as_deref(),
        app.home_art.loading,
        app.tick,
        palette().border,
    );

    if let Some(mix) = app.selected_home_mix() {
        let below = Rect::new(
            area.x,
            frame.bottom(),
            frame.width,
            area.height.saturating_sub(frame.height),
        );
        if below.height > 0 {
            f.render_widget(
                Paragraph::new(mix.title.as_str())
                    .style(
                        Style::default()
                            .fg(palette().accent)
                            .add_modifier(Modifier::BOLD),
                    )
                    .wrap(Wrap { trim: true })
                    .alignment(Alignment::Center),
                below,
            );
        }
    }
}

pub(super) fn render_home_section(
    f: &mut Frame,
    app: &App,
    section: &crate::app::HomeSection<crate::api::models::Playlist>,
    area: Rect,
) {
    if section.loading {
        let text = format!("{} Loading...", spinner_char(app.tick));
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(palette().dim)),
            area,
        );
        return;
    }

    if let Some(ref error) = section.error {
        f.render_widget(
            Paragraph::new(format!("Error: {error}")).style(Style::default().fg(palette().error)),
            area,
        );
        return;
    }

    if section.items.is_empty() {
        f.render_widget(
            Paragraph::new("No items").style(Style::default().fg(palette().dim)),
            area,
        );
        return;
    }

    let height = area.height as usize;
    let start = section.selected.saturating_sub(height.saturating_sub(1));

    let visible_items: Vec<ListItem> = section.items[start..]
        .iter()
        .take(height)
        .enumerate()
        .map(|(i, item)| {
            let is_selected = start + i == section.selected;

            let title_style = row_style(is_selected);

            ListItem::new(simple_row(
                app,
                &item.title,
                area.width,
                is_selected,
                title_style,
                "",
            ))
        })
        .collect();

    f.render_widget(List::new(visible_items), area);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Playlist;
    use crate::app::test_support::test_app;
    use ratatui::{Terminal, backend::TestBackend};

    fn mix(n: usize, name: &str) -> Playlist {
        Playlist {
            uuid: format!("uuid-{n}"),
            title: name.to_string(),
            number_of_tracks: None,
            description: None,
            cover: None,
            added_at: None,
            playlist_type: None,
        }
    }

    /// The counts here are what the live endpoints return: one new-release mix,
    /// eight daily mixes, one discovery mix.
    fn home_app() -> App {
        let mut t = test_app();
        t.app.home_new_releases.items = vec![mix(0, "My New Arrivals")];
        t.app.home_daily_mixes.items = (1..=8).map(|n| mix(n, &format!("My Mix {n}"))).collect();
        t.app.home_discovery_mixes.items = vec![mix(9, "My Daily Discovery")];
        t.app.home_new_releases.loading = false;
        t.app.home_daily_mixes.loading = false;
        t.app.home_discovery_mixes.loading = false;
        std::mem::forget(t.api_rx);
        t.app
    }

    fn top_row(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_home(f, app, Rect::new(0, 0, w, h)))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..w)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect()
    }

    /// On a short window the art is limited by height, not by its column; the
    /// width it cannot use belongs to the mixes, not to a gap beside the frame.
    #[test]
    fn the_mixes_start_where_the_art_frame_ends() {
        let (w, h) = (160u16, 16u16);
        let strip = top_row(&home_app(), w, h);
        let cells: Vec<char> = strip.chars().collect();
        let frame_w = cells.iter().rposition(|&c| c == '▁').unwrap() + 2;
        let list_x = cells.iter().position(|&c| c == '┌').unwrap();
        assert!(frame_w < (w / 4) as usize, "frame should be height-limited");
        assert_eq!(list_x, frame_w + HOME_ART_GAP as usize, "{strip}");
    }

    #[test]
    fn the_tab_strip_names_every_section_and_its_count() {
        let strip = top_row(&home_app(), 100, 20);
        assert!(strip.contains("New Releases (1)"), "{strip}");
        assert!(strip.contains("Daily Mixes (8)"), "{strip}");
        assert!(strip.contains("Daily Discovery (1)"), "{strip}");
    }

    #[test]
    fn the_art_column_yields_when_the_strip_needs_the_width() {
        let app = home_app();
        assert!(top_row(&app, 100, 20).contains('▁'));
        assert!(!top_row(&app, 60, 14).contains('▁'));
    }
}
