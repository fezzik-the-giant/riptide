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

    let hint = Span::styled(get_context_hint(app), Style::default().fg(DIM));
    let line = match update_notice(app) {
        Some(notice) => Line::from(vec![
            Span::styled(
                notice,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            hint,
        ]),
        None => Line::from(hint),
    };
    f.render_widget(Paragraph::new(line).alignment(Alignment::Left), cols[0]);

    let help_span = Span::styled(
        "? show keybinds",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(
        Paragraph::new(Line::from(help_span)).alignment(Alignment::Right),
        cols[1],
    );
}

/// Leads the footer so a narrow terminal cuts the context hint, not this.
fn update_notice(app: &App) -> Option<String> {
    let tag = app.update.available.as_deref()?;
    let method = app.update.install_method?;
    // Done means the binary on disk is already the new one; the notice has to
    // switch from "install this" to "you are still running the old one".
    Some(if app.update.status == crate::app::UpdateStatus::Done {
        format!("Updated to {tag} — restart to finish")
    } else {
        format!("Riptide {tag} available — {}", method.upgrade_hint())
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::test_app;
    use crate::update::InstallMethod;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn the_update_notice_survives_a_narrow_footer() {
        let mut t = test_app();
        t.app.update.available = Some("v9.9.9".into());
        t.app.update.install_method = Some(InstallMethod::Script);
        let notice = "Riptide v9.9.9 available — press U to upgrade";
        let w = notice.chars().count() as u16 + 20;

        let mut terminal = Terminal::new(TestBackend::new(w, 1)).unwrap();
        terminal
            .draw(|f| render_footer(f, &t.app, Rect::new(0, 0, w, 1)))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let row: String = (0..w)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect();

        assert!(row.starts_with(notice), "footer was {row:?}");
    }

    #[test]
    fn a_package_managed_install_is_pointed_at_its_package_manager() {
        let mut t = test_app();
        t.app.update.available = Some("v9.9.9".into());
        t.app.update.install_method = Some(InstallMethod::Pacman);

        assert_eq!(
            update_notice(&t.app).as_deref(),
            Some("Riptide v9.9.9 available — update with your AUR helper")
        );
    }
}
