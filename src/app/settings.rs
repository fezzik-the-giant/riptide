// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Reading and changing the settings the modal exposes.

use super::{App, Setting, SettingValue, StatusLevel, View};
use crate::lastfm::LastfmCmd;
use crate::visualizer::{SpectrumState, VisualizerMode};

impl App {
    /// Push a detail view, bringing its lists into line with the Atmos setting.
    ///
    /// Every view goes through here because its lists are built empty, long
    /// before the rows arrive — one opened after Atmos was turned off would
    /// otherwise show the very rows the rest of the app is hiding.
    pub(crate) fn push_view(&mut self, view: View) {
        // A response can push a view while a row is held; drop it back first,
        // or the playlist underneath keeps an order Tidal never got.
        self.cancel_move();
        self.view_stack.push(view);
        self.apply_atmos_visibility();
    }

    /// Drop the tracks the Atmos setting hides.
    ///
    /// The library lists narrow in place, keeping their rows and mapping around
    /// them. The search panes cannot: they are plain `Vec`s that their selection
    /// indexes directly, so a row left in but not drawn would make Enter act on
    /// the wrong track. Theirs are dropped as they arrive instead.
    pub(crate) fn shown_tracks(
        &self,
        tracks: Vec<crate::api::models::Track>,
    ) -> Vec<crate::api::models::Track> {
        if self.atmos {
            return tracks;
        }
        tracks.into_iter().filter(|t| !t.is_atmos_only()).collect()
    }

    /// [`Self::shown_tracks`] for the search albums pane.
    pub(crate) fn shown_albums(
        &self,
        albums: Vec<crate::api::models::Album>,
    ) -> Vec<crate::api::models::Album> {
        if self.atmos {
            return albums;
        }
        albums.into_iter().filter(|a| !a.is_atmos_only()).collect()
    }

    /// Mirror the Atmos setting into every list that can hold a mix, the views
    /// already on the stack included. Lists do not track the setting themselves;
    /// this is the one place that tells them, and telling one twice is free.
    pub(crate) fn apply_atmos_visibility(&mut self) {
        let hide = !self.atmos;
        self.favorites.set_hide_atmos(hide);
        self.fav_albums.set_hide_atmos(hide);
        for view in &mut self.view_stack {
            match view {
                View::Artist(detail) => {
                    detail.tracks.set_hide_atmos(hide);
                    detail.albums.set_hide_atmos(hide);
                    detail.eps.set_hide_atmos(hide);
                    detail.singles.set_hide_atmos(hide);
                }
                View::Album(detail) => detail.tracks.set_hide_atmos(hide),
                View::Playlist(detail) => detail.tracks.set_hide_atmos(hide),
            }
        }
    }
    pub fn setting_value(&self, setting: Setting) -> SettingValue {
        match setting {
            Setting::Atmos => SettingValue::Toggle(self.atmos),
            Setting::Shuffle => SettingValue::Toggle(self.now_playing.shuffle),
            Setting::Repeat => SettingValue::Choice(self.now_playing.repeat.label().into()),
            Setting::QueuePanel => SettingValue::Toggle(self.queue_visible),
            Setting::Visualizer => SettingValue::Choice(self.visualizer_mode.label().into()),
            Setting::Theme => SettingValue::Choice(
                self.theme_catalog
                    .find(&self.theme)
                    .map_or("Default", |entry| entry.name.as_str())
                    .to_string()
                    .into(),
            ),
            Setting::Volume => SettingValue::Percent(self.now_playing.volume),
            Setting::Scrobbling if !self.lastfm_configured => {
                SettingValue::Unavailable("run riptide --lastfm-auth")
            }
            Setting::Scrobbling => SettingValue::Toggle(self.lastfm_enabled),
        }
    }

    /// Flip the selected row. Volume has no two states to flip between, so
    /// Enter on it is a no-op and the arrows do the work.
    pub(crate) fn toggle_setting(&mut self, setting: Setting) {
        match self.setting_value(setting) {
            SettingValue::Toggle(on) => self.set_setting(setting, !on),
            SettingValue::Choice(_) => self.step_choice(setting, true),
            SettingValue::Percent(_) => {}
            SettingValue::Unavailable(reason) => {
                self.set_status(format!("{}: {reason}", setting.label()), StatusLevel::Info)
            }
        }
    }

    /// `←`/`→` on the selected row: a step of volume, or the off/on ends of a toggle.
    pub(crate) fn nudge_setting(&mut self, setting: Setting, forward: bool) {
        const VOLUME_STEP: u8 = 5;
        match self.setting_value(setting) {
            SettingValue::Percent(current) => {
                let next = if forward {
                    current.saturating_add(VOLUME_STEP).min(100)
                } else {
                    current.saturating_sub(VOLUME_STEP)
                };
                self.set_volume_percent(next);
            }
            SettingValue::Toggle(on) if on != forward => self.set_setting(setting, forward),
            SettingValue::Choice(_) => self.step_choice(setting, forward),
            _ => {}
        }
    }

    fn set_setting(&mut self, setting: Setting, on: bool) {
        match setting {
            Setting::Atmos => self.set_atmos(on),
            // Both already own the announcing and the side effects — shuffle
            // reorders the queue, and hiding the panel has to release focus.
            Setting::Shuffle => self.set_shuffle(on),
            Setting::QueuePanel => {
                if self.queue_visible != on {
                    self.toggle_queue_visible();
                }
            }
            Setting::Repeat | Setting::Visualizer | Setting::Theme | Setting::Volume => {}
            Setting::Scrobbling => {
                self.lastfm_enabled = on;
                let _ = self.lastfm_tx.send(LastfmCmd::SetEnabled(on));
                self.set_status(
                    format!("Last.fm scrobbling {}", if on { "on" } else { "off" }),
                    StatusLevel::Info,
                );
            }
        }
    }

    fn step_choice(&mut self, setting: Setting, forward: bool) {
        match setting {
            Setting::Visualizer => {
                let mode = self.visualizer_mode;
                self.set_visualizer_mode(if forward {
                    mode.next()
                } else {
                    mode.previous()
                });
            }
            Setting::Theme => {
                let next = self.theme_catalog.step(&self.theme, forward).to_string();
                self.set_theme(&next);
            }
            Setting::Repeat => {
                let mode = self.now_playing.repeat;
                self.set_repeat(if forward {
                    mode.next()
                } else {
                    mode.previous()
                });
            }
            _ => {}
        }
    }

    /// Read the themes folder again, so a file saved since the last look
    /// shows up without a restart.
    pub(crate) fn reload_themes(&mut self) {
        if let Some(dir) = &self.themes_dir {
            self.theme_catalog = crate::colors::catalog::ThemeCatalog::load(dir);
        }
    }

    pub(crate) fn open_theme_picker(&mut self) {
        self.reload_themes();
        let picker = &mut self.theme_picker;
        picker.active = true;
        picker.query.clear();
        picker.original = self.theme.clone();
        picker.selected = picker
            .matches(&self.theme_catalog)
            .iter()
            .position(|entry| entry.name.to_lowercase() == self.theme.to_lowercase())
            .unwrap_or(0);
    }

    /// Repaint in whatever the cursor is on; called after every move or
    /// keystroke, which is what makes the picker a preview.
    pub(crate) fn preview_selected_theme(&mut self) {
        if let Some(name) = self.theme_picker.selected_name(&self.theme_catalog) {
            self.preview_theme(&name);
        }
    }

    pub(crate) fn keep_selected_theme(&mut self) {
        if let Some(name) = self.theme_picker.selected_name(&self.theme_catalog) {
            self.set_theme(&name);
            self.theme_picker.active = false;
        }
    }

    pub(crate) fn cancel_theme_picker(&mut self) {
        let original = std::mem::take(&mut self.theme_picker.original);
        self.preview_theme(&original);
        self.theme_picker.active = false;
    }

    pub(crate) fn set_theme(&mut self, name: &str) {
        self.preview_theme(name);
        self.set_status(format!("Theme: {}", self.theme), StatusLevel::Info);
    }

    /// [`Self::set_theme`] without the announcement, for the picker, which
    /// changes theme on every keystroke.
    pub(crate) fn preview_theme(&mut self, name: &str) {
        self.theme = self
            .theme_catalog
            .find(name)
            .map_or_else(|| name.to_string(), |entry| entry.name.clone());
        // Only a cover in hand recomputes: after a restart the queued track's
        // cover is not fetched until it plays, and its saved colours are the
        // better answer than none.
        if self.is_album_theme()
            && let Some(cover) = self.now_playing_cover().map(<[u8]>::to_vec)
        {
            self.set_album_art(Some(&cover));
        }
    }

    /// Only crossing Off starts or stops CAVA: switching between two drawn
    /// modes is a render-side change, and restarting the child for it would
    /// blank the spectrum for the length of CAVA's start-up.
    ///
    /// A failed capture is the exception. The worker parks after a failure
    /// rather than respawning blindly, so any mode change re-sends `true` to
    /// wake it — that is what makes `v` a retry.
    pub(crate) fn set_visualizer_mode(&mut self, mode: VisualizerMode) {
        let was_on = self.visualizer_mode != VisualizerMode::Off;
        let is_on = mode != VisualizerMode::Off;
        let retrying = is_on
            && matches!(
                *self.visualizer.spectrum_rx.borrow(),
                SpectrumState::Unavailable
            );
        self.visualizer_mode = mode;
        if was_on != is_on || retrying {
            let _ = self.visualizer.enabled_tx.send(is_on);
        }
        let verb = if retrying {
            "Retrying visualizer"
        } else {
            "Visualizer"
        };
        self.set_status(format!("{verb}: {}", mode.label()), StatusLevel::Info);
    }

    /// The playing track keeps the stream it already resolved — swapping it
    /// would mean a silent round-trip and a seek back. The prefetched next one
    /// has not been heard yet, so it is re-resolved under the new setting.
    ///
    /// `next_prefetched` is deliberately left alone: it describes what mpv
    /// actually holds, and mpv goes on holding the old URL until `SetNext`
    /// swaps in the replacement. Clearing it here would read as a divergence to
    /// `TrackEnded` and replay a track that was advancing perfectly well.
    fn set_atmos(&mut self, on: bool) {
        if self.atmos == on {
            return;
        }
        self.atmos = on;
        self.apply_atmos_visibility();
        self.refresh_search();
        if let Some(next) = self.now_playing.next_up_id() {
            self.resolve_prefetch(next);
        }
        self.set_status(
            format!(
                "Dolby Atmos {} — applies from the next track",
                if on { "on" } else { "off" }
            ),
            StatusLevel::Info,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiRequest;
    use crate::app::RepeatMode;
    use crate::app::test_support::{solid_png, test_app, track, track_tagged};
    use crate::colors::Palette;
    use ratatui::style::Color;

    fn playing_with_cover(t: &mut crate::app::test_support::TestApp) -> u64 {
        t.app.now_playing.track = Some(track(1));
        let album_id = t.app.now_playing.track.as_ref().unwrap().album.id;
        t.app
            .handle_api_response(crate::api::ApiResponse::AlbumArt {
                album_id,
                image_data: solid_png(0xd2, 0x69, 0x1e),
            });
        album_id
    }

    #[test]
    fn album_mode_without_a_track_is_default() {
        let mut t = test_app();
        t.app.set_theme("Album");
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);
    }

    #[test]
    fn album_palette_survives_a_track_change_until_new_art() {
        let mut t = test_app();
        t.app.theme = "Album".into();
        playing_with_cover(&mut t);
        let themed = t.app.active_palette();
        assert_ne!(themed, Palette::DEFAULT);

        t.app.now_playing.track = Some(track(2));
        t.app.now_playing.set_art_bytes(None);
        t.app.now_playing.art_loading = true;
        assert_eq!(t.app.active_palette(), themed);

        t.app
            .handle_api_response(crate::api::ApiResponse::TrackArtFailed {
                track_id: 2,
                error: "gone".into(),
            });
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);
    }

    #[test]
    fn album_with_no_cover_drops_the_previous_palette() {
        let mut t = test_app();
        t.app.theme = "Album".into();
        let album_id = playing_with_cover(&mut t);
        t.app.now_playing.track = Some(track(2));

        let album = t.app.now_playing.track.as_ref().unwrap().album.clone();
        assert_eq!(album.id, album_id);
        assert!(album.cover.is_none());
        t.app
            .handle_api_response(crate::api::ApiResponse::AlbumLoaded { album });
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);
    }

    #[test]
    fn failed_album_lookup_drops_the_previous_palette() {
        let mut t = test_app();
        t.app.theme = "Album".into();
        let album_id = playing_with_cover(&mut t);
        t.app.now_playing.track = Some(track(2));

        t.app
            .handle_api_response(crate::api::ApiResponse::AlbumLoadFailed {
                album_id,
                error: "gone".into(),
            });
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);
    }

    /// Favourite tracks carry no album id or cover; the cover only arrives with
    /// the track details. Treating "no cover yet" as "no cover" flashed the
    /// default between every pair of tracks.
    #[test]
    fn a_track_without_a_cover_id_keeps_the_palette_until_its_details_arrive() {
        let favourite = |id| {
            let mut t = track(id);
            t.album.id = 0;
            t.album.cover = None;
            t
        };
        let mut t = test_app();
        t.app.theme = "Album".into();
        t.app.now_playing.track = Some(favourite(1));
        t.app.fetch_now_playing_metadata();
        t.app
            .handle_api_response(crate::api::ApiResponse::TrackDetails {
                track_id: 1,
                track: favourite(1),
                cover_url: Some("https://resources.tidal.com/images/one/320x320.jpg".into()),
            });
        t.app
            .handle_api_response(crate::api::ApiResponse::TrackArt {
                track_id: 1,
                image_data: solid_png(0xd2, 0x69, 0x1e),
            });
        let themed = t.app.active_palette();
        assert_ne!(themed, Palette::DEFAULT);

        let next = favourite(2);
        t.app.now_playing.track = Some(next.clone());
        t.app.fetch_now_playing_metadata();
        assert_eq!(
            t.app.active_palette(),
            themed,
            "flashed the default before the details"
        );

        t.app
            .handle_api_response(crate::api::ApiResponse::TrackDetails {
                track_id: 2,
                track: next,
                cover_url: None,
            });
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);
    }

    #[test]
    fn switching_to_album_mid_track_recolours_at_once() {
        let mut t = test_app();
        playing_with_cover(&mut t);
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);

        t.app.set_theme("Album");
        assert_ne!(t.app.active_palette(), Palette::DEFAULT);
    }

    #[test]
    fn fixed_theme_palette_is_the_files() {
        let mut t = test_app();
        t.app.set_theme("cyberpunk");
        assert_eq!(t.app.theme, "Cyberpunk");
        assert_eq!(
            t.app.active_palette().background,
            Some(Color::Rgb(0x10, 0x10, 0x14))
        );
    }

    #[test]
    fn unknown_saved_theme_draws_default_and_keeps_the_name() {
        let mut t = test_app();
        t.app.theme = "gone".into();
        assert_eq!(t.app.active_palette(), Palette::DEFAULT);
        assert_eq!(t.app.theme, "gone");
        assert!(matches!(
            t.app.setting_value(Setting::Theme),
            SettingValue::Choice(label) if label == "Default"
        ));
    }

    #[test]
    fn atmos_setting_rides_along_with_stream_requests() {
        let mut t = test_app();
        t.app.now_playing.queue = vec![track(1), track(2)];
        t.app.now_playing.queue_index = 0;
        t.drain_api();

        t.app.toggle_setting(Setting::Atmos);
        assert!(t.app.atmos);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::ResolveStreamUrl {
                track_id: 2,
                atmos: true,
                prefetch: true
            }]
        ));

        t.app.toggle_setting(Setting::Atmos);
        assert!(!t.app.atmos);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::ResolveStreamUrl {
                track_id: 2,
                atmos: false,
                prefetch: true
            }]
        ));
    }

    #[test]
    fn toggling_atmos_leaves_the_playing_track_alone() {
        let mut t = test_app();
        t.app.now_playing.queue = vec![track(1)];
        t.app.now_playing.queue_index = 0;
        t.app.now_playing.track = Some(track(1));
        t.drain_api();

        t.app.toggle_setting(Setting::Atmos);

        let requests = t.api_requests();
        assert!(
            !requests
                .iter()
                .any(|r| matches!(r, ApiRequest::ResolveStreamUrl { track_id: 1, .. })),
            "the playing track must not be re-resolved under the new setting"
        );
        assert!(t.app.now_playing.track.is_some());
    }

    #[test]
    fn turning_atmos_off_hides_the_rows_that_would_play_as_stereo() {
        let mut t = test_app();
        t.app.atmos = true;
        t.app.apply_atmos_visibility();
        t.app.favorites.items = vec![
            track_tagged(1, &["LOSSLESS"]),
            track_tagged(2, &["DOLBY_ATMOS"]),
            track_tagged(3, &["DOLBY_ATMOS", "LOSSLESS"]),
        ];
        t.app.favorites.refilter();
        assert_eq!(t.app.favorites.visible_len(), 3);

        t.app.toggle_setting(Setting::Atmos);

        // 2 is Atmos-only and goes; 3 carries a stereo mix and stays.
        let visible: Vec<u64> = t
            .app
            .favorites
            .visible_items()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(visible, [1, 3]);

        t.app.toggle_setting(Setting::Atmos);
        assert_eq!(t.app.favorites.visible_len(), 3);
    }

    /// A view built after the setting changed starts with empty lists, so it
    /// has to be brought into line as it is pushed rather than at load time.
    #[test]
    fn a_view_opened_later_honours_the_setting() {
        use crate::app::{AlbumDetail, StatefulList, View};
        let mut t = test_app();
        assert!(!t.app.atmos);

        t.app.push_view(View::Album(AlbumDetail {
            album: track(1).album,
            tracks: StatefulList::default(),
            art_bytes: None,
            art_loading: false,
        }));
        let Some(View::Album(detail)) = t.app.view_stack.last_mut() else {
            panic!("album detail was not pushed");
        };
        detail.tracks.items = vec![
            track_tagged(1, &["DOLBY_ATMOS"]),
            track_tagged(2, &["LOSSLESS"]),
        ];
        detail.tracks.refilter();

        let Some(View::Album(detail)) = t.app.view_stack.last() else {
            unreachable!()
        };
        assert_eq!(detail.tracks.visible_len(), 1);
        assert_eq!(detail.tracks.selected_item().map(|t| t.id), Some(2));
    }

    /// Search panes drop hidden rows on arrival instead of narrowing, so the
    /// only way to bring them into line is to ask Tidal again.
    #[test]
    fn toggling_atmos_re_runs_a_search_that_is_on_screen() {
        let mut t = test_app();
        t.app.search.query = "bohemian".to_string();
        t.app.search.tracks = vec![track(1)];
        t.drain_api();

        t.app.toggle_setting(Setting::Atmos);

        let requests = t.api_requests();
        assert!(
            requests
                .iter()
                .any(|r| matches!(r, ApiRequest::SearchTracks { .. })),
            "expected the query to be re-run, got {requests:?}"
        );
    }

    #[test]
    fn a_search_that_was_never_run_is_not_re_run() {
        let mut t = test_app();
        t.drain_api();

        t.app.toggle_setting(Setting::Atmos);

        assert!(
            !t.api_requests()
                .iter()
                .any(|r| matches!(r, ApiRequest::SearchTracks { .. }))
        );
    }

    #[test]
    fn scrobbling_row_refuses_to_turn_on_without_credentials() {
        let mut t = test_app();
        assert!(!t.app.lastfm_configured);

        t.app.toggle_setting(Setting::Scrobbling);

        assert!(matches!(
            t.app.setting_value(Setting::Scrobbling),
            SettingValue::Unavailable(_)
        ));
        assert!(t.app.status.is_some());
    }

    #[test]
    fn volume_nudges_by_five_and_clamps() {
        let mut t = test_app();
        t.app.set_volume_percent(98);

        t.app.nudge_setting(Setting::Volume, true);
        assert_eq!(t.app.now_playing.volume, 100);

        for _ in 0..25 {
            t.app.nudge_setting(Setting::Volume, false);
        }
        assert_eq!(t.app.now_playing.volume, 0);
    }

    #[test]
    fn switching_between_drawn_modes_leaves_capture_running() {
        let mut t = test_app();

        t.app.toggle_setting(Setting::Visualizer);
        assert_eq!(t.app.visualizer_mode, VisualizerMode::Bars);
        assert!(*t.visualizer_enabled_rx.borrow_and_update());

        t.app.nudge_setting(Setting::Visualizer, true);
        assert_eq!(t.app.visualizer_mode, VisualizerMode::Outline);
        assert!(!t.visualizer_enabled_rx.has_changed().unwrap());

        t.app.nudge_setting(Setting::Visualizer, false);
        t.app.nudge_setting(Setting::Visualizer, false);
        assert_eq!(t.app.visualizer_mode, VisualizerMode::Off);
        assert!(!*t.visualizer_enabled_rx.borrow_and_update());
        assert_eq!(t.app.preferences().visualizer_mode, VisualizerMode::Off);
    }

    #[test]
    fn repeat_steps_through_off_queue_and_track_and_is_saved() {
        let mut t = test_app();

        t.app.toggle_setting(Setting::Repeat);
        assert_eq!(t.app.now_playing.repeat, RepeatMode::Queue);
        t.app.cycle_repeat();
        assert_eq!(t.app.now_playing.repeat, RepeatMode::Track);
        t.app.nudge_setting(Setting::Repeat, true);
        assert_eq!(t.app.now_playing.repeat, RepeatMode::Off);
        t.app.nudge_setting(Setting::Repeat, false);

        assert_eq!(t.app.preferences().repeat, RepeatMode::Track);
    }

    #[test]
    fn a_failed_capture_is_reported_and_the_next_mode_change_retries_it() {
        let mut t = test_app();
        t.app.set_visualizer_mode(VisualizerMode::Bars);
        assert!(*t.visualizer_enabled_rx.borrow_and_update());

        t.spectrum_tx.send(SpectrumState::Unavailable).unwrap();
        t.app.tick();
        assert!(matches!(&t.app.status, Some((_, StatusLevel::Error, _))));

        t.app.set_visualizer_mode(VisualizerMode::Outline);
        assert!(t.visualizer_enabled_rx.has_changed().unwrap());
        assert!(*t.visualizer_enabled_rx.borrow_and_update());
    }
}
