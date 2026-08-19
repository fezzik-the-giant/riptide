// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! List and view navigation for the main content area.

use crossterm::event::{KeyCode, KeyEvent};

use super::*;
use crate::app::{App, ArtistDetailFocus, Tab, View};
use crate::playlist::PlaylistDetailFocus;
use crate::search::SearchPane;

pub(super) fn handle_navigation(app: &mut App, key: KeyEvent) {
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
            View::ArtistDetail(detail) => match key.code {
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
                        || detail.focus == ArtistDetailFocus::Singles
                    {
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
            },
            View::PlaylistDetail(detail) => match key.code {
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
                    if detail
                        .playlist
                        .description
                        .as_ref()
                        .map_or(false, |d| !d.is_empty())
                    {
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
            },
            View::AlbumDetail(detail) => {
                match key.code {
                    KeyCode::Up => {
                        detail.tracks.prev();
                        return;
                    }
                    KeyCode::Down => {
                        detail.tracks.next();
                        return;
                    }
                    KeyCode::PageUp => {
                        detail.tracks.page_up();
                        return;
                    }
                    KeyCode::PageDown => {
                        detail.tracks.page_down();
                        return;
                    }
                    KeyCode::Right | KeyCode::Char('l') => Action::FocusQueue,
                    KeyCode::Enter => {
                        let idx = detail.tracks.selected;
                        let mut tracks = detail.tracks.items.clone();
                        // Populate album cover from album detail
                        if let Some(cover) = &detail.album.cover {
                            tracing::debug!(
                                "Album cover from detail ({}): {}",
                                cover.starts_with("http"),
                                cover
                            );
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
                    KeyCode::Char('c') => match detail.tracks.items.get(detail.tracks.selected) {
                        Some(t) => Action::CopyUrl(t.share_url()),
                        None => return,
                    },
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
        Action::PlayTracks(tracks, idx) => {
            app.play_tracks(tracks, idx);
            return;
        }
        Action::OpenAlbum => {
            kitty_delete_album_art();
            kitty_delete_artist_art();
            app.open_selected_album();
            return;
        }
        Action::AddToQueue(track) => {
            app.add_to_queue(track);
            return;
        }
        Action::ToggleFavoriteTrack(track) => {
            app.toggle_favorite_track(&track);
            return;
        }
        Action::ToggleFollowArtist(artist) => {
            app.toggle_follow_artist(&artist);
            return;
        }
        Action::ToggleFavoriteAlbum(album) => {
            app.toggle_favorite_album(&album);
            return;
        }
        Action::TrackRadio(track) => {
            app.start_track_radio(&track);
            return;
        }
        Action::ArtistRadio(artist) => {
            app.start_artist_radio(&artist);
            return;
        }
        Action::FocusQueue => {
            app.focus_queue();
            return;
        }
        Action::PlayPlaylistTracks(tracks, idx, uuid) => {
            app.play_playlist_tracks(tracks, idx, uuid);
            return;
        }
        Action::CopyUrl(url) => {
            app.copy_url(url);
            return;
        }
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
            Tab::Favorites => app.favorites.next(),
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
        KeyCode::Right | KeyCode::Char('l')
            if !matches!(app.current_tab, Tab::Home | Tab::Search) =>
        {
            app.focus_queue();
        }
        KeyCode::Enter => match app.current_tab {
            Tab::Home => app.open_selected_home_item(),
            Tab::Artists => app.open_selected_artist(),
            Tab::Albums => app.open_selected_fav_album(),
            Tab::Playlists => app.open_selected_playlist(),
            Tab::Favorites => {
                // Queue what is on screen: `selected` indexes the visible rows,
                // so with a filter applied the unfiltered list would play the
                // wrong track.
                let idx = app.favorites.selected;
                let tracks: Vec<_> = (0..app.favorites.visible_len())
                    .filter_map(|i| app.favorites.get_visible(i).cloned())
                    .collect();
                if !tracks.is_empty() {
                    app.play_tracks(tracks, idx);
                }
            }
            Tab::Search => match app.search.pane {
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
            },
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
        // Every row in a library tab is already saved, so `f` there could only ever
        // remove — a destructive action on a home-row key that people kept firing
        // by accident (#39). Removal moved to `d`, which already means "remove" in
        // the queue; `f` now says so rather than silently doing nothing.
        KeyCode::Char('f') => match app.current_tab {
            Tab::Artists | Tab::Albums | Tab::Playlists | Tab::Favorites => {
                app.set_status(
                    "Already in your library — press d to remove".to_string(),
                    crate::app::StatusLevel::Info,
                );
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
        KeyCode::Char('d') => match app.current_tab {
            Tab::Artists => {
                if let Some(artist) = app.artists.selected_item().cloned() {
                    app.unfollow_artist(&artist);
                }
            }
            Tab::Playlists => {
                if let Some(playlist) = app.playlists.selected_item().cloned() {
                    app.remove_playlist(&playlist);
                }
            }
            Tab::Albums => {
                if let Some(album) = app.fav_albums.selected_item().cloned() {
                    app.unfavorite_album(&album);
                }
            }
            Tab::Favorites => {
                if let Some(track) = app.favorites.selected_item().cloned() {
                    app.unfavorite_track(&track);
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
                if let Some(url) = app
                    .search
                    .tracks
                    .get(app.search.track_sel)
                    .map(|t| t.share_url())
                {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Artists => {
                if let Some(url) = app
                    .search
                    .artists
                    .get(app.search.artist_sel)
                    .map(|a| a.share_url())
                {
                    app.copy_url(url);
                }
            }
            Tab::Search if app.search.pane == SearchPane::Playlists => {
                if let Some(url) = app
                    .search
                    .playlists
                    .get(app.search.playlist_sel)
                    .map(|p| p.share_url())
                {
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
                if let Some(url) = app
                    .search
                    .tracks
                    .get(app.search.track_sel)
                    .map(|t| t.album.share_url())
                {
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

pub(super) fn get_selected_track(app: &App) -> Option<crate::api::models::Track> {
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
