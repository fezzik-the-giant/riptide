// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! The Home tab: new releases, daily mixes and discovery mixes.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::*;
use crate::app::{App, HomeSectionFocus};

/// Narrowest art column worth drawing into. Below this the cover is a smudge
/// and the mixes are better off with the width.
const HOME_ART_MIN_W: u16 = 12;

pub(super) fn render_home(f: &mut Frame, app: &App, area: Rect) {
    let labels = [
        section_label(app, HomeSectionFocus::NewReleases),
        section_label(app, HomeSectionFocus::DailyMixes),
        section_label(app, HomeSectionFocus::DiscoveryMixes),
    ];

    // The art only gets a column once the tab strip has the room it needs;
    // otherwise the section names truncate away and navigation loses its map.
    let art_w = area
        .width
        .saturating_sub(carousel_width(&labels))
        .min(area.width / 4);
    let list_area = if art_w >= HOME_ART_MIN_W {
        let cols = Layout::horizontal([Constraint::Length(art_w), Constraint::Min(0)]).split(area);
        render_home_art(f, app, cols[0]);
        cols[1]
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

fn render_home_art(f: &mut Frame, app: &App, area: Rect) {
    // Covers are square, and `Resize::Fit` keeps them that way, so the frame has
    // to be square in *pixels* — which takes the terminal's real cell size, not
    // an assumed one. Whole cells rarely divide evenly, so the art is fitted to
    // the smaller axis and the box drawn around what it actually occupies.
    let (cell_w, cell_h) = cell_size();
    let title_rows = 2;
    let mut cols = area.width.saturating_sub(2);
    let mut rows = (cols as u32 * cell_w as u32 / cell_h as u32) as u16;
    let mut max_rows = area.height.saturating_sub(2 + title_rows);
    // The art never scales up, so the frame must not outgrow it.
    if let Some((art_cols, art_rows)) = app.home_art.bytes.as_deref().and_then(image_cells) {
        cols = cols.min(art_cols);
        max_rows = max_rows.min(art_rows);
    }
    if rows > max_rows {
        rows = max_rows;
    }
    // Back-solve the width from the rows that survived rounding, so the frame is
    // the size the fitted image ends up rather than the size it asked for.
    cols = cols.min((rows as u32 * cell_h as u32 / cell_w as u32) as u16);
    if cols == 0 || rows == 0 {
        return;
    }

    let frame = Rect::new(area.x, area.y, cols + 2, rows + 2);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(frame);
    f.render_widget(block, frame);

    if let Some(bytes) = &app.home_art.bytes {
        render_image(f, bytes, inner);
    } else if app.home_art.loading {
        f.render_widget(
            Paragraph::new(spinner_char(app.tick).to_string())
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Center),
            inner,
        );
    }

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
                    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
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
        f.render_widget(Paragraph::new(text).style(Style::default().fg(DIM)), area);
        return;
    }

    if let Some(ref error) = section.error {
        f.render_widget(
            Paragraph::new(format!("Error: {error}")).style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    if section.items.is_empty() {
        f.render_widget(
            Paragraph::new("No items").style(Style::default().fg(DIM)),
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

            let title_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .bg(HIGHLIGHT_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

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

    /// `Resize::Fit` never scales up, so a cover smaller than the space available
    /// renders at native size — the frame has to shrink to it or the border
    /// floats clear of the picture.
    #[test]
    fn the_art_frame_shrinks_to_a_small_cover() {
        let mut app = home_app();
        let (cell_w, cell_h) = cell_size();
        let (px_w, px_h) = (cell_w as u32 * 4, cell_h as u32 * 4);
        let mut png = Vec::new();
        ::image::DynamicImage::ImageRgba8(::image::RgbaImage::new(px_w, px_h))
            .write_to(
                &mut std::io::Cursor::new(&mut png),
                ::image::ImageFormat::Png,
            )
            .unwrap();
        app.home_art.bytes = Some(png);

        let (w, h) = (140u16, 30u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_home(f, &app, Rect::new(0, 0, w, h)))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let row = |y: u16| -> String {
            (0..w)
                .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                .collect()
        };

        let cols = row(0).chars().position(|c| c == '┐').unwrap() + 1;
        let rows = (0..h).find(|&y| row(y).starts_with('└')).unwrap() + 1;
        assert_eq!(
            (cols - 2, rows - 2),
            (4, 4),
            "frame should hug the 4x4 cover"
        );
    }

    /// The cover is square and `Resize::Fit` keeps it that way, so the frame has
    /// to be square in pixels or the border floats clear of the picture.
    #[test]
    fn the_art_frame_is_square_in_pixels() {
        let app = home_app();
        let (cell_w, cell_h) = cell_size();

        for (w, h) in [(96u16, 24u16), (140, 30), (96, 12)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| render_home(f, &app, Rect::new(0, 0, w, h)))
                .unwrap();
            let buf = term.backend().buffer().clone();

            let row = |y: u16| -> String {
                (0..w)
                    .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                    .collect()
            };
            let cols = row(0).chars().position(|c| c == '┐').unwrap() + 1;
            let rows = (0..h).find(|&y| row(y).starts_with('└')).unwrap() + 1;

            let (px_w, px_h) = (
                (cols as u16 - 2) * cell_w,
                (rows.saturating_sub(2)) * cell_h,
            );
            assert_eq!(px_w, px_h, "{w}x{h}: frame is {px_w}x{px_h} px");
        }
    }

    #[test]
    fn the_tab_strip_names_every_section_and_its_count() {
        let strip = top_row(&home_app(), 100, 20);
        assert!(strip.contains("New Releases (1)"), "{strip}");
        assert!(strip.contains("Daily Mixes (8)"), "{strip}");
        assert!(strip.contains("Daily Discovery (1)"), "{strip}");
    }

    /// Two boxes on the top row means the art got a column, one means it was
    /// dropped so the strip keeps its labels.
    #[test]
    fn the_art_column_yields_when_the_strip_needs_the_width() {
        let app = home_app();
        assert_eq!(top_row(&app, 100, 20).matches('┌').count(), 2);
        assert_eq!(top_row(&app, 60, 14).matches('┌').count(), 1);
    }
}
