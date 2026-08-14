// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Full-window presentation of the current album artwork.

use ratatui::{
    Frame,
    layout::{Alignment, Rect, Size},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::FontSize;

use super::*;
use crate::app::App;

const ART_HUD_HEIGHT: u16 = 5;
const MAX_ART_EDGE_PIXELS: u32 = 640;

pub(super) fn render_art_view(f: &mut Frame, app: &App, area: Rect) {
    let (canvas, hud) = art_view_layout(area);
    render_art(f, app, canvas);
    render_art_hud(f, app, hud);
}

fn render_art(f: &mut Frame, app: &App, area: Rect) {
    if let Some((bytes, content_hash)) = art_image(app) {
        let art_area = centered_square_art_area(area, get_picker().font_size());
        render_scaled_image(f, bytes, art_area, content_hash);
        return;
    }

    if area.is_empty() {
        return;
    }
    let message = if app.now_playing.art_loading || app.now_playing.presentation_art_loading {
        spinner_char(app.tick).to_string()
    } else {
        "No artwork".to_string()
    };
    f.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(DIM))
            .alignment(Alignment::Center),
        Rect::new(area.x, area.y + area.height / 2, area.width, 1),
    );
}

fn art_image(app: &App) -> Option<(&[u8], u64)> {
    app.now_playing
        .presentation_art_image()
        .or_else(|| app.now_playing.art_image())
}

fn render_art_hud(f: &mut Frame, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (title, metadata) = match &app.now_playing.track {
        Some(track) => (
            track.title.as_str(),
            format!("{} · {}", track.all_artist_names(), track.album.title),
        ),
        None => ("No track playing", String::new()),
    };

    render_centered_row(
        f,
        inner,
        0,
        title,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    render_centered_row(f, inner, 1, &metadata, Style::default().fg(DIM));

    if let Some(progress_area) = inset_row(inner, 2, 2) {
        f.render_widget(progress_rail(app, progress_area.width), progress_area);
    }

    if let Some(status_area) = inset_row(inner, 3, 2) {
        let time = format!(
            "{} / {}",
            app.now_playing.position_display(),
            app.now_playing.duration_display(),
        );
        let left_width = status_area.width / 2;
        let left = Rect::new(status_area.x, status_area.y, left_width, 1);
        let right = Rect::new(
            status_area.x + left_width,
            status_area.y,
            status_area.width.saturating_sub(left_width),
            1,
        );
        f.render_widget(Paragraph::new(time).style(Style::default().fg(DIM)), left);
        f.render_widget(
            Paragraph::new(format!("Volume: {}%", app.now_playing.volume))
                .style(Style::default().fg(DIM))
                .alignment(Alignment::Right),
            right,
        );
    }
}

fn render_centered_row(f: &mut Frame, area: Rect, offset: u16, text: &str, style: Style) {
    if offset >= area.height {
        return;
    }
    f.render_widget(
        Paragraph::new(text)
            .style(style)
            .alignment(Alignment::Center),
        Rect::new(area.x, area.y + offset, area.width, 1),
    );
}

fn inset_row(area: Rect, offset: u16, inset: u16) -> Option<Rect> {
    if offset >= area.height {
        return None;
    }
    let inset = inset.min(area.width / 2);
    Some(Rect::new(
        area.x + inset,
        area.y + offset,
        area.width.saturating_sub(inset.saturating_mul(2)),
        1,
    ))
}

fn progress_rail(app: &App, width: u16) -> Paragraph<'static> {
    let played = progress_columns(width, app.now_playing.progress_ratio());
    Paragraph::new(Line::from(vec![
        Span::styled("━".repeat(played as usize), Style::default().fg(ACCENT)),
        Span::styled(
            "━".repeat(width.saturating_sub(played) as usize),
            Style::default().fg(DIM),
        ),
    ]))
}

fn progress_columns(width: u16, ratio: f64) -> u16 {
    ((width as f64 * ratio.clamp(0.0, 1.0)) as u16).min(width)
}

fn art_view_layout(area: Rect) -> (Rect, Rect) {
    let hud_height = ART_HUD_HEIGHT.min(area.height);
    let canvas_height = area.height.saturating_sub(hud_height);
    (
        Rect::new(area.x, area.y, area.width, canvas_height),
        Rect::new(area.x, area.y + canvas_height, area.width, hud_height),
    )
}

/// Finds the largest cell rectangle whose pixel dimensions are square for the
/// terminal's detected font, then centers it in the available canvas.
fn centered_square_art_area(area: Rect, font_size: FontSize) -> Rect {
    if area.is_empty() || font_size.width == 0 || font_size.height == 0 {
        return Rect::new(area.x, area.y, 0, 0);
    }

    let available_width = u32::from(area.width) * u32::from(font_size.width);
    let available_height = u32::from(area.height) * u32::from(font_size.height);
    let edge = available_width
        .min(available_height)
        .min(MAX_ART_EDGE_PIXELS);
    let width = edge
        .div_ceil(u32::from(font_size.width))
        .min(u32::from(area.width)) as u16;
    let height = edge
        .div_ceil(u32::from(font_size.height))
        .min(u32::from(area.height)) as u16;

    centered_protocol_area(area, Size::new(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Album, ArtistRef, Track};
    use crate::app::test_app;
    use ratatui::{Terminal, backend::TestBackend};

    fn app_with_track() -> App {
        let mut app = test_app().0;
        app.now_playing.track = Some(Track {
            id: 1,
            title: "HUD Track".to_string(),
            duration: 240,
            artist: Some(ArtistRef {
                name: "HUD Artist".to_string(),
            }),
            artists: Vec::new(),
            album: Album {
                id: 2,
                title: "HUD Album".to_string(),
                number_of_tracks: None,
                release_date: None,
                cover: None,
                artist: None,
                audio_quality: None,
                media_metadata: None,
                added_at: None,
                album_type: None,
            },
            audio_quality: None,
            media_metadata: None,
            added_at: None,
        });
        app.now_playing.position = 60.0;
        app.now_playing.duration = 240.0;
        app.now_playing.volume = 73;
        app
    }

    #[test]
    fn art_view_reserves_only_its_five_row_hud() {
        let (canvas, hud) = art_view_layout(Rect::new(5, 7, 120, 40));
        assert_eq!(canvas, Rect::new(5, 7, 120, 35));
        assert_eq!(hud, Rect::new(5, 42, 120, 5));
    }

    #[test]
    fn presentation_art_takes_priority_over_the_thumbnail() {
        let mut app = app_with_track();
        app.now_playing.set_art_bytes(Some(vec![3, 2, 0]));
        assert_eq!(
            art_image(&app).map(|(bytes, _)| bytes),
            Some([3, 2, 0].as_slice())
        );

        app.now_playing
            .set_presentation_art_bytes(Some(vec![12, 8, 0]));
        assert_eq!(
            art_image(&app).map(|(bytes, _)| bytes),
            Some([12, 8, 0].as_slice())
        );
    }

    #[test]
    fn art_uses_detected_cell_dimensions_when_terminal_is_wide() {
        assert_eq!(
            centered_square_art_area(Rect::new(5, 7, 120, 40), FontSize::new(8, 16)),
            Rect::new(25, 7, 80, 40),
        );
    }

    #[test]
    fn art_uses_detected_cell_dimensions_when_terminal_is_tall() {
        assert_eq!(
            centered_square_art_area(Rect::new(5, 7, 70, 50), FontSize::new(8, 16)),
            Rect::new(5, 14, 70, 35),
        );
    }

    #[test]
    fn art_surface_is_capped_at_a_640_pixel_edge() {
        assert_eq!(
            centered_square_art_area(Rect::new(0, 0, 300, 100), FontSize::new(8, 16)),
            Rect::new(110, 30, 80, 40),
        );
    }

    #[test]
    fn art_layout_handles_empty_and_single_column_areas() {
        let font_size = FontSize::new(8, 16);
        assert!(centered_square_art_area(Rect::new(0, 0, 0, 10), font_size).is_empty());
        assert_eq!(
            centered_square_art_area(Rect::new(0, 0, 1, 10), font_size),
            Rect::new(0, 4, 1, 1),
        );
        assert!(centered_square_art_area(Rect::new(0, 0, 10, 0), font_size).is_empty());
    }

    #[test]
    fn progress_columns_are_clamped_to_the_rail() {
        assert_eq!(progress_columns(20, -1.0), 0);
        assert_eq!(progress_columns(20, 0.5), 10);
        assert_eq!(progress_columns(20, 2.0), 20);
        assert_eq!(progress_columns(0, 0.5), 0);
    }

    #[test]
    fn hud_contains_track_metadata_time_and_volume() {
        let app = app_with_track();
        let mut terminal = Terminal::new(TestBackend::new(80, ART_HUD_HEIGHT)).unwrap();
        terminal
            .draw(|f| render_art_hud(f, &app, f.area()))
            .unwrap();

        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("HUD Track"));
        assert!(rendered.contains("HUD Artist · HUD Album"));
        assert!(rendered.contains("1:00 / 4:00"));
        assert!(rendered.contains("Volume: 73%"));
    }

    #[test]
    fn art_view_omits_the_generic_footer() {
        let mut app = app_with_track();
        app.art_fullscreen = true;
        app.now_playing.set_art_bytes(None);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &app)).unwrap();

        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("HUD Track"));
        assert!(!rendered.contains("show keybinds"));
    }
}
