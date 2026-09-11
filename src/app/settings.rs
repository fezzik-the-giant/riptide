// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Fezzik the Giant

//! Reading and changing the settings the modal exposes.

use super::{App, Setting, SettingValue, StatusLevel, View};
use crate::lastfm::LastfmCmd;

impl App {
    /// Push a detail view, bringing its lists into line with the Atmos setting.
    ///
    /// Every view goes through here because its lists are built empty, long
    /// before the rows arrive — one opened after Atmos was turned off would
    /// otherwise show the very rows the rest of the app is hiding.
    pub(crate) fn push_view(&mut self, view: View) {
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

    /// Mirror the Atmos setting into every list that can hold a mix, the views
    /// already on the stack included. Lists do not track the setting themselves;
    /// this is the one place that tells them, and telling one twice is free.
    pub(crate) fn apply_atmos_visibility(&mut self) {
        let hide = !self.atmos;
        self.favorites.set_hide_atmos(hide);
        self.fav_albums.set_hide_atmos(hide);
        for view in &mut self.view_stack {
            match view {
                View::ArtistDetail(detail) => {
                    detail.tracks.set_hide_atmos(hide);
                    detail.albums.set_hide_atmos(hide);
                    detail.eps.set_hide_atmos(hide);
                    detail.singles.set_hide_atmos(hide);
                }
                View::AlbumDetail(detail) => detail.tracks.set_hide_atmos(hide),
                View::PlaylistDetail(detail) => detail.tracks.set_hide_atmos(hide),
            }
        }
    }
    pub fn setting_value(&self, setting: Setting) -> SettingValue {
        match setting {
            Setting::Atmos => SettingValue::Toggle(self.atmos),
            Setting::Shuffle => SettingValue::Toggle(self.now_playing.shuffle),
            Setting::QueuePanel => SettingValue::Toggle(self.queue_visible),
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
            Setting::Volume => {}
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
        if let Some(next) = self.now_playing.queue.get(self.now_playing.queue_index + 1) {
            self.resolve_stream(next.id);
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
    use crate::app::test_support::{test_app, track, track_tagged};

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
                atmos: true
            }]
        ));

        t.app.toggle_setting(Setting::Atmos);
        assert!(!t.app.atmos);
        assert!(matches!(
            t.api_requests().as_slice(),
            [ApiRequest::ResolveStreamUrl {
                track_id: 2,
                atmos: false
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

        t.app.push_view(View::AlbumDetail(AlbumDetail {
            album: track(1).album,
            tracks: StatefulList::default(),
            art_bytes: None,
            art_loading: false,
        }));
        let Some(View::AlbumDetail(detail)) = t.app.view_stack.last_mut() else {
            panic!("album detail was not pushed");
        };
        detail.tracks.items = vec![
            track_tagged(1, &["DOLBY_ATMOS"]),
            track_tagged(2, &["LOSSLESS"]),
        ];
        detail.tracks.refilter();

        let Some(View::AlbumDetail(detail)) = t.app.view_stack.last() else {
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
}
