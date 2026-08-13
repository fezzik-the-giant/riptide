// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::api::{ApiRequest, ApiResponse};
use crate::app::{App, ArtistDetailFocus, SortPalette, Tab, View};
use crate::search::SearchPane;
use crate::playlist::PlaylistDetailFocus;
use crate::mpris::MprisCmd;
use crate::player::{PlayerCmd, PlayerEvent};

pub fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
    mut api_rx: mpsc::UnboundedReceiver<ApiResponse>,
    mut player_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    mut mpris_rx: mpsc::UnboundedReceiver<MprisCmd>,
    lastfm_evt_tx: mpsc::UnboundedSender<PlayerEvent>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| crate::ui::draw(f, app))?;

        // Drain API responses
        while let Ok(resp) = api_rx.try_recv() {
            app.handle_api_response(resp);
        }

        // Drain player events and forward to Last.fm
        while let Ok(evt) = player_rx.try_recv() {
            let _ = lastfm_evt_tx.send(evt.clone());
            app.handle_player_event(evt);
        }

        // Drain MPRIS control commands
        while let Ok(cmd) = mpris_rx.try_recv() {
            match cmd {
                MprisCmd::Next => app.next_track(),
                MprisCmd::Previous => app.prev_track(),
                MprisCmd::Play => app.set_paused(false),
                MprisCmd::Pause => app.set_paused(true),
                MprisCmd::PlayPause => app.toggle_pause(),
                MprisCmd::Stop => { let _ = app.player_tx.send(PlayerCmd::Stop); }
            }
        }

        // Check for more data to load
        check_load_more(app);

        app.tick();

        if app.should_quit {
            break;
        }

        // Poll for key events with a short timeout to keep animations smooth
        // Drain all pending key events and only process the last one to avoid lag from key repeat
        let mut last_key_event: Option<KeyEvent> = None;
        while event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                last_key_event = Some(key);
            }
        }
        if let Some(key) = last_key_event {
            handle_key(app, key);
        }

        // Small delay to keep animations smooth
        if !event::poll(Duration::from_millis(16))? {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(())
}

fn kitty_delete_album_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=2\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

fn kitty_delete_artist_art() {
    if std::env::var("KITTY_WINDOW_ID").is_ok() {
        use std::io::Write;
        print!("\x1b_Ga=d,d=i,i=3\x1b\\");
        let _ = std::io::stdout().flush();
    }
}

fn leaving_album(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::AlbumDetail(_)))
}

fn leaving_artist(app: &App) -> bool {
    matches!(app.view_stack.last(), Some(View::ArtistDetail(_)))
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if app.help_active {
        handle_help_input(app, key);
        return;
    }

    if app.queue_focused {
        handle_queue_input(app, key);
        return;
    }

    if app.command.active {
        handle_command_input(app, key);
        return;
    }

    if app.sort_palette.active {
        handle_sort_palette_input(app, key);
        return;
    }

    if app.artist_selection.active {
        handle_artist_selection_input(app, key);
        return;
    }

    // Search overlay captures all keys while active or modal is open, regardless of current tab.
    if app.search.active || app.search.modal_open {
        handle_search_input(app, key);
        return;
    }

    // Open search modal when on Search tab
    if app.current_tab == Tab::Search {
        if key.code == KeyCode::Char('/') {
            app.search.modal_open = true;
            app.search.query.clear();
            return;
        }
    }

    // Global bindings
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        KeyCode::Char('?') => {
            app.help_active = true;
            app.help_scroll = 0;
        }
        KeyCode::Char('/') => {
            app.command.active = true;
            app.command.input.clear();
            app.command.selected = 0;
        }
        KeyCode::Tab => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_tab();
            } else {
                app.next_tab();
            }
        }
        KeyCode::BackTab => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.prev_tab();
        }
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('n') => app.next_track(),
        KeyCode::Char('p') => app.prev_track(),
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Esc => {
            if leaving_album(app) { kitty_delete_album_art(); }
            if leaving_artist(app) { kitty_delete_artist_art(); }
            app.go_back();
        }
        KeyCode::Char('+') | KeyCode::Char('=') => { let _ = app.player_tx.send(PlayerCmd::ChangeVolume(5)); },
        KeyCode::Char('-') => { let _ = app.player_tx.send(PlayerCmd::ChangeVolume(-5)); },
        KeyCode::Char('g') => {
            if let Some(track) = get_selected_track(app) {
                app.go_to_artist_from_track(&track);
            } else {
                app.set_status("No track selected".to_string(), crate::app::StatusLevel::Error);
            }
        }
        _ => handle_navigation(app, key),
    }
}

fn handle_command_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.command.active = false;
        }
        KeyCode::Enter => {
            let matches = app.command.matches();
            let cmd = matches.get(app.command.selected)
                .or_else(|| matches.first())
                .copied();
            if let Some(cmd) = cmd {
                execute_command(app, cmd);
            } else {
                app.command.active = false;
            }
        }
        KeyCode::Tab => {
            // Accept ghost-text completion.
            if let Some(&first) = app.command.matches().first() {
                app.command.input = first.to_string();
                app.command.selected = 0;
            }
        }
        KeyCode::Up => {
            if app.command.selected > 0 {
                app.command.selected -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.command.matches().len();
            if app.command.selected + 1 < len {
                app.command.selected += 1;
            }
        }
        KeyCode::Backspace => {
            app.command.input.pop();
            app.command.selected = 0;
        }
        KeyCode::Char(c) => {
            app.command.input.push(c);
            app.command.selected = 0;
        }
        _ => {}
    }
}

fn execute_command(app: &mut App, cmd: &str) {
    app.command.active = false;
    app.command.input.clear();
    let cleanup = |app: &App| {
        if leaving_album(app) { kitty_delete_album_art(); }
        if leaving_artist(app) { kitty_delete_artist_art(); }
    };
    match cmd {
        "home" => {
            cleanup(app);
            app.set_tab(Tab::Home);
        }
        "favorites" => {
            cleanup(app);
            app.set_tab(Tab::Favorites);
        }
        "artists" => {
            cleanup(app);
            app.set_tab(Tab::Artists);
        }
        "albums" => {
            cleanup(app);
            app.set_tab(Tab::Albums);
        }
        "playlists" => {
            cleanup(app);
            app.set_tab(Tab::Playlists);
        }
        "search" => {
            cleanup(app);
            app.set_tab(Tab::Search);
            app.search.modal_open = true;
            app.search.query.clear();
        }
        _ => {}
    }
}

fn handle_sort_palette_input(app: &mut App, key: KeyEvent) {
    let options = SortPalette::get_options(app.current_tab);
    let count = options.len();
    match key.code {
        KeyCode::Esc => {
            app.sort_palette.active = false;
        }
        KeyCode::Up => {
            if app.sort_palette.selected > 0 {
                app.sort_palette.selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.sort_palette.selected + 1 < count {
                app.sort_palette.selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some((_, field)) = options.get(app.sort_palette.selected){
                app.apply_sort(*field);
            }
        }
        _ => {}
    }
}

fn handle_help_input(app: &mut App, key: KeyEvent) {
    use crate::app::KeybindGroup;

    let max_scroll = {
        let total_lines = KeybindGroup::total_help_lines() as i16;
        let visible_lines = 22i16; // Modal inner height (24 - 2 for borders) in render_help_modal
        (total_lines - visible_lines).max(0) as u16
    };

    match key.code {
        KeyCode::Esc => {
            app.help_active = false;
            app.help_scroll = 0;
        }
        KeyCode::Up => {
            app.help_scroll = app.help_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            app.help_scroll = (app.help_scroll + 1).min(max_scroll);
        }
        KeyCode::PageUp => {
            app.help_scroll = app.help_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.help_scroll = (app.help_scroll + 10).min(max_scroll);
        }
        _ => {}
    }
}

fn handle_navigation(app: &mut App, key: KeyEvent) {
    // First pass: mutate the view's own list state (navigation within a detail view).
    // Collect any "play" action as data so we can call app methods after the borrow ends.
    enum Action {
        None,
        PlayTracks(Vec<crate::api::models::Track>, usize),
        OpenAlbum,
        AddToQueue(crate::api::models::Track),
        ToggleFavoriteTrack(crate::api::models::Track),
        ToggleFollowArtist(crate::api::models::Artist),
        ToggleFavoriteAlbum(crate::api::models::Album),
        TrackRadio(crate::api::models::Track),
        ArtistRadio(crate::api::models::Artist),
        FocusQueue,
        PlayPlaylistTracks(Vec<crate::api::models::Track>, usize, String),
        CopyUrl(String),
    }

    let action: Action = if let Some(view) = app.view_stack.last_mut() {
        match view {
            View::ArtistDetail(detail) => {
                match key.code {
                    KeyCode::Up => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.prev(),
                            ArtistDetailFocus::Albums => detail.albums.prev(),
                            ArtistDetailFocus::EPs => detail.eps.prev(),
                            ArtistDetailFocus::Singles => detail.singles.prev(),
                            ArtistDetailFocus::Bio => {
                                detail.bio_scroll = detail.bio_scroll.saturating_sub(1);
                            }
                        }
                        return;
                    }
                    KeyCode::Down => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.next(),
                            ArtistDetailFocus::Albums => detail.albums.next(),
                            ArtistDetailFocus::EPs => detail.eps.next(),
                            ArtistDetailFocus::Singles => detail.singles.next(),
                            ArtistDetailFocus::Bio => {
                                detail.bio_scroll = detail.bio_scroll.saturating_add(1);
                            }
                        }
                        return;
                    }
                    KeyCode::PageUp => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.page_up(),
                            ArtistDetailFocus::Albums => detail.albums.page_up(),
                            ArtistDetailFocus::EPs => detail.eps.page_up(),
                            ArtistDetailFocus::Singles => detail.singles.page_up(),
                            ArtistDetailFocus::Bio => {}
                        }
                        return;
                    }
                    KeyCode::PageDown => {
                        match detail.focus {
                            ArtistDetailFocus::Tracks => detail.tracks.page_down(),
                            ArtistDetailFocus::Albums => detail.albums.page_down(),
                            ArtistDetailFocus::EPs => detail.eps.page_down(),
                            ArtistDetailFocus::Singles => detail.singles.page_down(),
                            ArtistDetailFocus::Bio => {}
                        }
                        return;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        detail.focus = match detail.focus {
                            ArtistDetailFocus::Tracks => ArtistDetailFocus::Bio,
                            ArtistDetailFocus::Albums => ArtistDetailFocus::Tracks,
                            ArtistDetailFocus::EPs => ArtistDetailFocus::Albums,
                            ArtistDetailFocus::Singles => ArtistDetailFocus::EPs,
                            ArtistDetailFocus::Bio => ArtistDetailFocus::Singles,
                        };
                        return;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if detail.focus == ArtistDetailFocus::Singles {
                            Action::FocusQueue
                        } else {
                            detail.focus = match detail.focus {
                                ArtistDetailFocus::Bio => ArtistDetailFocus::Tracks,
                                ArtistDetailFocus::Tracks => ArtistDetailFocus::Albums,
                                ArtistDetailFocus::Albums => ArtistDetailFocus::EPs,
                                ArtistDetailFocus::EPs => ArtistDetailFocus::Singles,
                                ArtistDetailFocus::Singles => unreachable!(),
                            };
                            return;
                        }
                    }
                    KeyCode::Enter => {
                        if detail.focus == ArtistDetailFocus::Tracks {
                            let idx = detail.tracks.selected;
                            let tracks = detail.tracks.items.clone();
                            Action::PlayTracks(tracks, idx)
                        } else if detail.focus == ArtistDetailFocus::Albums
                            || detail.focus == ArtistDetailFocus::EPs
                            || detail.focus == ArtistDetailFocus::Singles {
                            Action::OpenAlbum
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('a') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::ToggleFavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::Albums => {
                        match detail.albums.items.get(detail.albums.selected).cloned() {
                            Some(a) => Action::ToggleFavoriteAlbum(a),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::EPs => {
                        match detail.eps.items.get(detail.eps.selected).cloned() {
                            Some(a) => Action::ToggleFavoriteAlbum(a),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') if detail.focus == ArtistDetailFocus::Singles => {
                        match detail.singles.items.get(detail.singles.selected).cloned() {
                            Some(a) => Action::ToggleFavoriteAlbum(a),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => Action::ToggleFollowArtist(detail.artist.clone()),
                    KeyCode::Char('r') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => Action::ArtistRadio(detail.artist.clone()),
                    KeyCode::Char('c') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') if detail.focus == ArtistDetailFocus::Albums => {
                        match detail.albums.items.get(detail.albums.selected) {
                            Some(a) => Action::CopyUrl(a.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') if detail.focus == ArtistDetailFocus::EPs => {
                        match detail.eps.items.get(detail.eps.selected) {
                            Some(a) => Action::CopyUrl(a.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') if detail.focus == ArtistDetailFocus::Singles => {
                        match detail.singles.items.get(detail.singles.selected) {
                            Some(a) => Action::CopyUrl(a.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') => Action::CopyUrl(detail.artist.share_url()),
                    KeyCode::Char('C') if detail.focus == ArtistDetailFocus::Tracks => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.album.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('C') if detail.focus == ArtistDetailFocus::Albums => {
                        Action::CopyUrl(detail.artist.share_url())
                    }
                    KeyCode::Char('C') if detail.focus == ArtistDetailFocus::EPs => {
                        Action::CopyUrl(detail.artist.share_url())
                    }
                    KeyCode::Char('C') if detail.focus == ArtistDetailFocus::Singles => {
                        Action::CopyUrl(detail.artist.share_url())
                    }
                    _ => return,
                }
            }
            View::PlaylistDetail(detail) => {
                match key.code {
                    KeyCode::Up => {
                        match detail.focus {
                            PlaylistDetailFocus::Tracks => detail.tracks.prev(),
                            PlaylistDetailFocus::Description => {
                                detail.description_scroll = detail.description_scroll.saturating_sub(1);
                            }
                        }
                        return;
                    }
                    KeyCode::Down => {
                        match detail.focus {
                            PlaylistDetailFocus::Tracks => detail.tracks.next(),
                            PlaylistDetailFocus::Description => {
                                detail.description_scroll = detail.description_scroll.saturating_add(1);
                            }
                        }
                        return;
                    }
                    KeyCode::PageUp => {
                        match detail.focus {
                            PlaylistDetailFocus::Tracks => detail.tracks.page_up(),
                            PlaylistDetailFocus::Description => {}
                        }
                        return;
                    }
                    KeyCode::PageDown => {
                        match detail.focus {
                            PlaylistDetailFocus::Tracks => detail.tracks.page_down(),
                            PlaylistDetailFocus::Description => {}
                        }
                        return;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if detail.playlist.description.as_ref().map_or(false, |d| !d.is_empty()) {
                            detail.focus = match detail.focus {
                                PlaylistDetailFocus::Tracks => PlaylistDetailFocus::Description,
                                PlaylistDetailFocus::Description => PlaylistDetailFocus::Tracks,
                            };
                        }
                        return;
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if detail.focus == PlaylistDetailFocus::Tracks {
                            Action::FocusQueue
                        } else {
                            detail.focus = PlaylistDetailFocus::Tracks;
                            return;
                        }
                    }
                    KeyCode::Enter => {
                        if detail.focus == PlaylistDetailFocus::Tracks {
                            let idx = detail.tracks.selected;
                            let tracks = detail.tracks.items.clone();
                            let uuid = detail.playlist.uuid.clone();
                            Action::PlayPlaylistTracks(tracks, idx, uuid)
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('a') => {
                        if detail.focus == PlaylistDetailFocus::Tracks {
                            match detail.tracks.items.get(detail.tracks.selected).cloned() {
                                Some(t) => Action::AddToQueue(t),
                                None => return,
                            }
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('f') => {
                        if detail.focus == PlaylistDetailFocus::Tracks {
                            match detail.tracks.items.get(detail.tracks.selected).cloned() {
                                Some(t) => Action::ToggleFavoriteTrack(t),
                                None => return,
                            }
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('r') => {
                        if detail.focus == PlaylistDetailFocus::Tracks {
                            match detail.tracks.items.get(detail.tracks.selected).cloned() {
                                Some(t) => Action::TrackRadio(t),
                                None => return,
                            }
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('c') => {
                        if detail.focus == PlaylistDetailFocus::Tracks {
                            match detail.tracks.items.get(detail.tracks.selected) {
                                Some(t) => Action::CopyUrl(t.share_url()),
                                None => return,
                            }
                        } else {
                            return;
                        }
                    }
                    KeyCode::Char('C') => Action::CopyUrl(detail.playlist.share_url()),
                    _ => return,
                }
            }
            View::AlbumDetail(detail) => {
                match key.code {
                    KeyCode::Up => { detail.tracks.prev(); return; }
                    KeyCode::Down => { detail.tracks.next(); return; }
                    KeyCode::PageUp => { detail.tracks.page_up(); return; }
                    KeyCode::PageDown => { detail.tracks.page_down(); return; }
                    KeyCode::Right | KeyCode::Char('l') => Action::FocusQueue,
                    KeyCode::Enter => {
                        let idx = detail.tracks.selected;
                        let mut tracks = detail.tracks.items.clone();
                        // Populate album cover from album detail
                        if let Some(cover) = &detail.album.cover {
                            tracing::debug!("Album cover from detail ({}): {}", cover.starts_with("http"), cover);
                            for track in &mut tracks {
                                track.album.cover = Some(cover.clone());
                            }
                        }
                        Action::PlayTracks(tracks, idx)
                    }
                    KeyCode::Char('a') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::AddToQueue(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('f') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::ToggleFavoriteTrack(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('r') => {
                        match detail.tracks.items.get(detail.tracks.selected).cloned() {
                            Some(t) => Action::TrackRadio(t),
                            None => return,
                        }
                    }
                    KeyCode::Char('c') => {
                        match detail.tracks.items.get(detail.tracks.selected) {
                            Some(t) => Action::CopyUrl(t.share_url()),
                            None => return,
                        }
                    }
                    KeyCode::Char('C') => Action::CopyUrl(detail.album.share_url()),
                    _ => return,
                }
            }
        }
    } else {
        Action::None
    };

    // Apply any collected action (borrow of view_stack has ended)
    match action {
        Action::PlayTracks(tracks, idx) => { app.play_tracks(tracks, idx); return; }
        Action::OpenAlbum => { kitty_delete_album_art(); kitty_delete_artist_art(); app.open_selected_album(); return; }
        Action::AddToQueue(track) => { app.add_to_queue(track); return; }
        Action::ToggleFavoriteTrack(track) => { app.toggle_favorite_track(&track); return; }
        Action::ToggleFollowArtist(artist) => { app.toggle_follow_artist(&artist); return; }
        Action::ToggleFavoriteAlbum(album) => { app.toggle_favorite_album(&album); return; }
        Action::TrackRadio(track) => { app.start_track_radio(&track); return; }
        Action::ArtistRadio(artist) => { app.start_artist_radio(&artist); return; }
        Action::FocusQueue => { app.focus_queue(); return; }
        Action::PlayPlaylistTracks(tracks, idx, uuid) => { app.play_playlist_tracks(tracks, idx, uuid); return; }
        Action::CopyUrl(url) => { app.copy_url(url); return; }
        Action::None => {}
    }

    // Top-level tab navigation (no active detail view)
    match key.code {
        KeyCode::Up => match app.current_tab {
            Tab::Home => {
                use crate::app::HomeSectionFocus;
                match app.home_section_focus {
                    HomeSectionFocus::NewReleases => app.home_new_releases.prev(),
                    HomeSectionFocus::DailyMixes => app.home_daily_mixes.prev(),
                    HomeSectionFocus::DiscoveryMixes => app.home_discovery_mixes.prev(),
                }
            }
            Tab::Artists => app.artists.prev(),
            Tab::Albums => app.fav_albums.prev(),
            Tab::Playlists => app.playlists.prev(),
            Tab::Favorites => app.favorites.prev(),
            Tab::Search => app.search.pane_prev(),
        },
        KeyCode::Down => match app.current_tab {
            Tab::Home => {
                use crate::app::HomeSectionFocus;
                match app.home_section_focus {
                    HomeSectionFocus::NewReleases => app.home_new_releases.next(),
                    HomeSectionFocus::DailyMixes => app.home_daily_mixes.next(),
                    HomeSectionFocus::DiscoveryMixes => app.home_discovery_mixes.next(),
                }
            }
            Tab::Artists => app.artists.next(),
            Tab::Albums => app.fav_albums.next(),
            Tab::Playlists => app.playlists.next(),
            Tab::Favorites => {
                app.favorites.next();
                if app.favorites.should_load_more() {
                    app.load_favorites();
                }
            }
            Tab::Search => app.search.pane_next(),
        },
        KeyCode::PageUp => match app.current_tab {
            Tab::Home => {}
            Tab::Artists => app.artists.page_up(),
            Tab::Albums => app.fav_albums.page_up(),
            Tab::Playlists => app.playlists.page_up(),
            Tab::Favorites => app.favorites.page_up(),
            Tab::Search => app.search.pane_page_up(),
        },
        KeyCode::PageDown => match app.current_tab {
            Tab::Home => {}
            Tab::Artists => app.artists.page_down(),
            Tab::Albums => app.fav_albums.page_down(),
            Tab::Playlists => app.playlists.page_down(),
            Tab::Favorites => app.favorites.page_down(),
            Tab::Search => app.search.pane_page_down(),
        },
        KeyCode::Left | KeyCode::Char('h') if app.current_tab == Tab::Home => {
            use crate::app::HomeSectionFocus;
            app.home_section_focus = match app.home_section_focus {
                HomeSectionFocus::NewReleases => HomeSectionFocus::DiscoveryMixes,
                HomeSectionFocus::DailyMixes => HomeSectionFocus::NewReleases,
                HomeSectionFocus::DiscoveryMixes => HomeSectionFocus::DailyMixes,
            };
        }
        KeyCode::Left | KeyCode::Char('h') if app.current_tab == Tab::Search => {
            app.search.prev_pane();
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab == Tab::Home => {
            use crate::app::HomeSectionFocus;
            app.home_section_focus = match app.home_section_focus {
                HomeSectionFocus::NewReleases => HomeSectionFocus::DailyMixes,
                HomeSectionFocus::DailyMixes => HomeSectionFocus::DiscoveryMixes,
                HomeSectionFocus::DiscoveryMixes => HomeSectionFocus::NewReleases,
            };
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab == Tab::Search => {
            app.search.next_pane();
        }
        KeyCode::Right | KeyCode::Char('l') if app.current_tab != Tab::Search && app.current_tab != Tab::Home => {
            app.focus_queue();
        }
        KeyCode::Enter => match app.current_tab {
            Tab::Home => app.open_selected_home_item(),
            Tab::Artists => app.open_selected_artist(),
            Tab::Albums => app.open_selected_fav_album(),
            Tab::Playlists => app.open_selected_playlist(),
            Tab::Favorites => {
                let idx = app.favorites.selected;
                let tracks = app.favorites.items.clone();
                if !tracks.is_empty() {
                    app.play_tracks(tracks, idx);
                }
            }
            Tab::Search => {
                match app.search.pane {
                    SearchPane::Tracks => {
                        let idx = app.search.track_sel;
                        if let Some(track) = app.search.tracks.get(idx).cloned() {
                            app.play_track(track);
                        }
                    }
                    SearchPane::Artists => {
                        let idx = app.search.artist_sel;
                        if let Some(artist) = app.search.artists.get(idx).cloned() {
                            app.open_artist(artist);
                        }
                    }
                    SearchPane::Playlists => {
                        let idx = app.search.playlist_sel;
                        if let Some(playlist) = app.search.playlists.get(idx).cloned() {
                            app.open_playlist(playlist);
                        }
                    }
                }
            }
        },
        KeyCode::Char('a') => match app.current_tab {
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.add_to_queue(track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.add_to_queue(track);
                }
            }
            _ => {}
        },
        KeyCode::Char('f') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.toggle_follow_artist(&artist);
                }
            }
            Tab::Playlists => {
                if let Some(playlist) = app.playlists.selected_item().cloned() {
                    app.toggle_save_playlist(&playlist);
                }
            }
            Tab::Albums => {
                if let Some(album) = app.fav_albums.selected_item().cloned() {
                    app.toggle_favorite_album(&album);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.toggle_favorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.toggle_favorite_track(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.toggle_follow_artist(&artist);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Playlists => {
                if let Some(playlist) = app.search.playlists.get(app.search.playlist_sel).cloned() {
                    app.toggle_save_playlist(&playlist);
                }
            }
            _ => {}
        },
        KeyCode::Char('c') => match app.current_tab {
            Tab::Artists => {
                if let Some(url) = app.artists.selected_item().map(|a| a.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Albums => {
                if let Some(url) = app.fav_albums.selected_item().map(|a| a.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Playlists => {
                if let Some(url) = app.playlists.selected_item().map(|p| p.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Favorites => {
                if let Some(url) = app.favorites.selected_item().map(|t| t.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(url) = app.search.tracks.get(app.search.track_sel).map(|t| t.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(url) = app.search.artists.get(app.search.artist_sel).map(|a| a.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Playlists => {
                if let Some(url) = app.search.playlists.get(app.search.playlist_sel).map(|p| p.share_url()) {
                    app.copy_url(url);
                }
            }
            _ => {}
        },
        // Shift+C copies the *parent* of the selected item. Only tracks have a
        // reachable parent (their album); artist/album/playlist rows have no
        // parent id in the API models, so they no-op.
        KeyCode::Char('C') => match app.current_tab {
            Tab::Favorites => {
                if let Some(url) = app.favorites.selected_item().map(|t| t.album.share_url()) {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(url) = app.search.tracks.get(app.search.track_sel).map(|t| t.album.share_url()) {
                    app.copy_url(url);
                }
            }
            _ => {}
        },
        KeyCode::Char('s') => match app.current_tab {
            Tab::Favorites | Tab::Artists | Tab::Albums | Tab::Playlists => {
                if app.view_stack.is_empty() {
                    app.open_sort_palette();
                }
            }
            _ => {}
        },
        KeyCode::Char('r') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.start_artist_radio(&artist);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.start_track_radio(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Tracks => {
                if let Some(track) = app.search.tracks.get(app.search.track_sel).cloned() {
                    app.start_track_radio(&track);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(artist) = app.search.artists.get(app.search.artist_sel).cloned() {
                    app.start_artist_radio(&artist);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn handle_search_input(app: &mut App, key: KeyEvent) {
    // If modal is open, handle input for the search modal
    if app.search.modal_open {
        match key.code {
            KeyCode::Esc => {
                app.search.modal_open = false;
                app.prev_tab();
            }
            KeyCode::Enter => {
                let query = app.search.query.clone();
                app.search.modal_open = false;
                if !query.is_empty() {
                    app.search.loading = true;
                    app.search.tracks_awaiting_page2 = true;
                    app.search.artists_awaiting_page2 = true;
                    app.search.playlists_awaiting_page2 = true;
                    app.search.track_sel = 0;
                    app.search.artist_sel = 0;
                    app.search.playlist_sel = 0;
                    app.search.tracks.clear();
                    app.search.artists.clear();
                    app.search.playlists.clear();
                    app.search.reset_viewports();
                    app.search.pane = SearchPane::Tracks;
                    let _ = app.api_tx.send(ApiRequest::SearchTracks { query: query.clone() });
                    let _ = app.api_tx.send(ApiRequest::SearchArtistsMain { query: query.clone() });
                    let _ = app.api_tx.send(ApiRequest::SearchPlaylistsMain { query });
                }
            }
            KeyCode::Backspace => {
                app.search.query.pop();
            }
            KeyCode::Char(c) => {
                app.search.query.push(c);
            }
            _ => {}
        }
    } else {
        // Original behavior for search results navigation
        match key.code {
            KeyCode::Tab => {
                app.search.active = false;
                app.next_tab();
            }
            KeyCode::BackTab => {
                app.search.active = false;
                app.prev_tab();
            }
            KeyCode::Esc => {
                // Close overlay, stay on current view.
                app.search.active = false;
            }
            KeyCode::Enter => {
                let query = app.search.query.clone();
                app.search.active = false;
                if !query.is_empty() {
                    if leaving_album(app) { kitty_delete_album_art(); }
                    app.view_stack.clear();
                    app.current_tab = Tab::Search;
                    app.search.loading = true;
                    app.search.track_sel = 0;
                    app.search.artist_sel = 0;
                    app.search.playlist_sel = 0;
                    app.search.reset_viewports();
                    app.search.pane = SearchPane::Tracks;
                    let _ = app.api_tx.send(ApiRequest::SearchTracks { query: query.clone() });
                    let _ = app.api_tx.send(ApiRequest::SearchArtistsMain { query: query.clone() });
                    let _ = app.api_tx.send(ApiRequest::SearchPlaylistsMain { query });
                }
            }
            KeyCode::Backspace => {
                app.search.query.pop();
            }
            KeyCode::Char(c) => {
                app.search.query.push(c);
            }
            _ => {}
        }
    }
}

fn handle_queue_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.unfocus_queue();
        }
        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_queue_track_up();
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_queue_track_down();
        }
        KeyCode::Up => {
            if app.queue_cursor > 0 {
                app.queue_cursor -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.now_playing.queue.len();
            if len > 0 && app.queue_cursor + 1 < len {
                app.queue_cursor += 1;
            }
        }
        KeyCode::PageUp => app.queue_page_up(),
        KeyCode::PageDown => app.queue_page_down(),
        KeyCode::Enter => {
            let cursor = app.queue_cursor;
            app.play_from_queue(cursor);
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            let cursor = app.queue_cursor;
            app.remove_from_queue(cursor);
        }
        KeyCode::Char('f') => {
            if let Some(track) = app.now_playing.queue.get(app.queue_cursor).cloned() {
                app.toggle_favorite_track(&track);
            }
        }
        KeyCode::Char('c') => {
            if let Some(url) = app.now_playing.queue.get(app.queue_cursor).map(|t| t.share_url()) {
                app.copy_url(url);
            }
        }
        KeyCode::Char('C') => {
            if let Some(url) = app.now_playing.queue.get(app.queue_cursor).map(|t| t.album.share_url()) {
                app.copy_url(url);
            }
        }
        KeyCode::Char('z') => app.toggle_shuffle(),
        KeyCode::Char(' ') => app.toggle_pause(),
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            app.should_quit = true;
        }
        _ => {}
    }
}

fn get_selected_track(app: &App) -> Option<crate::api::models::Track> {
    if let Some(View::PlaylistDetail(detail)) = app.view_stack.last() {
        return detail.tracks.selected_item().cloned();
    }
    if let Some(View::ArtistDetail(detail)) = app.view_stack.last() {
        return detail.tracks.selected_item().cloned();
    }
    if app.current_tab == Tab::Favorites {
        return app.favorites.selected_item().cloned();
    }
    if app.now_playing.queue_index < app.now_playing.queue.len() {
        return Some(app.now_playing.queue[app.now_playing.queue_index].clone());
    }
    None
}

fn handle_artist_selection_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.artist_selection.active = false;
        }
        KeyCode::Up => {
            if app.artist_selection.selected > 0 {
                app.artist_selection.selected -= 1;
            }
        }
        KeyCode::Down => {
            let len = app.artist_selection.artist_names.len();
            if app.artist_selection.selected + 1 < len {
                app.artist_selection.selected += 1;
            }
        }
        KeyCode::Enter => {
            app.open_selected_artist_from_selection();
        }
        _ => {}
    }
}

fn check_load_more(app: &mut App) {
    // Detail view tracks — checked before tab-level lists so the guard below
    // (`view_stack.is_empty()`) doesn't shadow it.
    if let Some(View::PlaylistDetail(detail)) = app.view_stack.last() {
        if detail.tracks.should_load_more() {
            app.load_more_playlist_tracks();
            return;
        }
    }
    if let Some(View::ArtistDetail(detail)) = app.view_stack.last() {
        match detail.focus {
            ArtistDetailFocus::Tracks if detail.tracks.should_load_more() => {
                app.load_more_artist_tracks();
                return;
            }
            ArtistDetailFocus::Albums if detail.albums.should_load_more() => {
                app.load_more_artist_albums();
                return;
            }
            ArtistDetailFocus::EPs if detail.eps.should_load_more() => {
                app.load_more_artist_eps();
                return;
            }
            ArtistDetailFocus::Singles if detail.singles.should_load_more() => {
                app.load_more_artist_singles();
                return;
            }
            _ => {}
        }
    }

    match app.current_tab {
        Tab::Search if app.view_stack.is_empty() => {
            if app.search.should_load_more_for_pane() {
                match app.search.pane {
                    SearchPane::Tracks => app.load_search_tracks_next(),
                    SearchPane::Artists => app.load_search_artists_next(),
                    SearchPane::Playlists => app.load_search_playlists_next(),
                }
            }
        }
        Tab::Artists if app.view_stack.is_empty() => {
            if app.artists.should_load_more() {
                app.load_artists();
            }
        }
        Tab::Albums if app.view_stack.is_empty() => {
            if app.fav_albums.should_load_more() {
                app.load_fav_albums();
            }
        }
        Tab::Playlists if app.view_stack.is_empty() => {
            if app.playlists.should_load_more() {
                app.load_playlists();
            }
        }
        Tab::Favorites if app.view_stack.is_empty() => {
            if app.favorites.should_load_more() {
                app.load_favorites();
            }
        }
        _ => {}
    }
}
