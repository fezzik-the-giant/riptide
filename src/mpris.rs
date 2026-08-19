// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::object_server::SignalContext;
use zbus::zvariant::{Array, ObjectPath, OwnedValue, Signature, Str, Value};
use zbus::{connection, interface};

const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const BUS_NAME: &str = "org.mpris.MediaPlayer2.riptide";
const TRACK_PATH_PREFIX: &str = "/org/riptide/track/";
// Sentinel defined by the MPRIS spec for "no current track".
const NO_TRACK_PATH: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct MprisState {
    pub track_id: u64,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art_url: String,
    pub duration_us: i64,
    pub position_us: i64,
    /// Bumped on every discontinuous position change (a seek). Position moving
    /// forward at playback rate must not be signalled, so the `Seeked` signal
    /// keys off this counter instead of the position itself.
    pub position_epoch: u64,
    pub paused: bool,
    pub active: bool,
    /// mpv scale, 0–100. MPRIS speaks 0.0–1.0; the conversion lives at the interface.
    pub volume: u8,
    pub shuffle: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub has_track: bool,
}

fn playback_status(s: &MprisState) -> &'static str {
    if !s.active {
        "Stopped"
    } else if s.paused {
        "Paused"
    } else {
        "Playing"
    }
}

// ── Commands from MPRIS clients back to the app ───────────────────────────────

pub enum MprisCmd {
    Next,
    Previous,
    PlayPause,
    Play,
    Pause,
    Stop,
    Quit,
    /// MPRIS scale, 0.0–1.0.
    SetVolume(f64),
    SetShuffle(bool),
    /// Relative offset in microseconds.
    Seek(i64),
    /// Track id (parsed from the client's trackid path) and absolute position in
    /// microseconds. The id lets the app drop calls aimed at a track that has
    /// since changed, as the spec requires.
    SetPosition(u64, i64),
}

// ── D-Bus interfaces ──────────────────────────────────────────────────────────

struct RootIface {
    cmd_tx: mpsc::UnboundedSender<MprisCmd>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl RootIface {
    fn raise(&self) {}
    fn quit(&self) {
        let _ = self.cmd_tx.send(MprisCmd::Quit);
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        // A TUI cannot raise the terminal window it happens to run in.
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "Riptide"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec![]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![]
    }
}

struct PlayerIface {
    state: Arc<Mutex<MprisState>>,
    cmd_tx: mpsc::UnboundedSender<MprisCmd>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerIface {
    async fn next(&self) {
        let _ = self.cmd_tx.send(MprisCmd::Next);
    }
    async fn previous(&self) {
        let _ = self.cmd_tx.send(MprisCmd::Previous);
    }
    async fn pause(&self) {
        let _ = self.cmd_tx.send(MprisCmd::Pause);
    }
    async fn play_pause(&self) {
        let _ = self.cmd_tx.send(MprisCmd::PlayPause);
    }
    async fn stop(&self) {
        let _ = self.cmd_tx.send(MprisCmd::Stop);
    }
    async fn play(&self) {
        let _ = self.cmd_tx.send(MprisCmd::Play);
    }
    async fn seek(&self, offset: i64) {
        let _ = self.cmd_tx.send(MprisCmd::Seek(offset));
    }
    async fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        if let Some(id) = parse_track_path(track_id.as_str()) {
            let _ = self.cmd_tx.send(MprisCmd::SetPosition(id, position));
        }
    }
    async fn open_uri(&self, _uri: String) {}

    #[zbus(signal)]
    async fn seeked(ctxt: &SignalContext<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        playback_status(&self.state.lock().unwrap()).into()
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        "None".into()
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    // Set asynchronously: the command is queued and the value only lands once the
    // app pushes it back. zbus's automatic post-setter signal would read the getter
    // in the meantime and announce the *old* value, making a desktop volume slider
    // snap back before correcting. The server loop's diff is the only honest source.
    #[zbus(property(emits_changed_signal = "false"))]
    fn shuffle(&self) -> bool {
        self.state.lock().unwrap().shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&self, shuffle: bool) {
        let _ = self.cmd_tx.send(MprisCmd::SetShuffle(shuffle));
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        build_metadata(&self.state.lock().unwrap())
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn volume(&self) -> f64 {
        self.state.lock().unwrap().volume as f64 / 100.0
    }

    #[zbus(property)]
    fn set_volume(&self, volume: f64) {
        let _ = self.cmd_tx.send(MprisCmd::SetVolume(volume));
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.state.lock().unwrap().position_us
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state.lock().unwrap().can_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state.lock().unwrap().can_prev
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.state.lock().unwrap().has_track
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.state.lock().unwrap().has_track
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state.lock().unwrap().active
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn parse_track_path(path: &str) -> Option<u64> {
    path.strip_prefix(TRACK_PATH_PREFIX)?.parse().ok()
}

fn owned_str(s: &str) -> OwnedValue {
    OwnedValue::try_from(Value::Str(Str::from(s.to_owned()))).unwrap()
}

fn str_array_value(items: &[String]) -> OwnedValue {
    let sig = Signature::try_from("s").unwrap();
    let mut arr = Array::new(sig);
    for s in items {
        arr.append(Value::Str(Str::from(s.to_owned()))).ok();
    }
    OwnedValue::try_from(Value::Array(arr)).unwrap()
}

fn build_metadata(s: &MprisState) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();

    let path = if s.has_track && s.track_id > 0 {
        format!("{TRACK_PATH_PREFIX}{}", s.track_id)
    } else {
        NO_TRACK_PATH.to_owned()
    };
    let path: ObjectPath<'static> = ObjectPath::try_from(path).unwrap();
    map.insert(
        "mpris:trackid".into(),
        OwnedValue::try_from(Value::ObjectPath(path)).unwrap(),
    );

    if !s.title.is_empty() {
        map.insert("xesam:title".into(), owned_str(&s.title));
    }
    if !s.artists.is_empty() {
        map.insert("xesam:artist".into(), str_array_value(&s.artists));
    }
    if !s.album.is_empty() {
        map.insert("xesam:album".into(), owned_str(&s.album));
    }
    if !s.art_url.is_empty() {
        map.insert("mpris:artUrl".into(), owned_str(&s.art_url));
    }
    if s.duration_us > 0 {
        map.insert("mpris:length".into(), OwnedValue::from(s.duration_us));
    }

    map
}

// ── Change detection ──────────────────────────────────────────────────────────

/// Which signals a state transition warrants. Position advancing at playback
/// rate is the one change that must NOT produce a signal — the spec forbids
/// signalling `Position`, and the app pushes state on every 500 ms poll.
#[derive(Debug, Default, PartialEq)]
struct SignalPlan {
    status: bool,
    metadata: bool,
    volume: bool,
    shuffle: bool,
    can_next: bool,
    can_prev: bool,
    can_play_pause: bool,
    can_seek: bool,
    seeked: bool,
}

fn metadata_fields(s: &MprisState) -> (u64, &str, &[String], &str, &str, i64, bool) {
    (
        s.track_id,
        &s.title,
        &s.artists,
        &s.album,
        &s.art_url,
        s.duration_us,
        s.has_track,
    )
}

fn plan_signals(prev: &MprisState, new: &MprisState) -> SignalPlan {
    SignalPlan {
        status: playback_status(prev) != playback_status(new),
        metadata: metadata_fields(prev) != metadata_fields(new),
        volume: prev.volume != new.volume,
        shuffle: prev.shuffle != new.shuffle,
        can_next: prev.can_next != new.can_next,
        can_prev: prev.can_prev != new.can_prev,
        can_play_pause: prev.has_track != new.has_track,
        can_seek: prev.active != new.active,
        seeked: prev.position_epoch != new.position_epoch,
    }
}

// ── Server ────────────────────────────────────────────────────────────────────

pub struct MprisServer {
    state_rx: watch::Receiver<MprisState>,
    cmd_tx: mpsc::UnboundedSender<MprisCmd>,
}

impl MprisServer {
    pub fn new(
        state_rx: watch::Receiver<MprisState>,
        cmd_tx: mpsc::UnboundedSender<MprisCmd>,
    ) -> Self {
        Self { state_rx, cmd_tx }
    }

    pub async fn run(mut self) {
        let shared: Arc<Mutex<MprisState>> = Arc::new(Mutex::new(MprisState::default()));

        let conn = match connection::Builder::session()
            .and_then(|b| {
                b.serve_at(
                    OBJECT_PATH,
                    RootIface {
                        cmd_tx: self.cmd_tx.clone(),
                    },
                )
            })
            .and_then(|b| {
                b.serve_at(
                    OBJECT_PATH,
                    PlayerIface {
                        state: Arc::clone(&shared),
                        cmd_tx: self.cmd_tx.clone(),
                    },
                )
            }) {
            Ok(builder) => match builder.build().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("MPRIS disabled: could not connect to session bus: {e}");
                    return;
                }
            },
            Err(e) => {
                tracing::warn!("MPRIS disabled: {e}");
                return;
            }
        };

        // Claim the name without ReplaceExisting so a second riptide cannot
        // silently steal it from the first; the spec's answer to multiple
        // instances is a pid-suffixed name.
        let flags = RequestNameFlags::DoNotQueue.into();
        match conn.request_name_with_flags(BUS_NAME, flags).await {
            Ok(RequestNameReply::PrimaryOwner) => {}
            _ => {
                let fallback = format!("{BUS_NAME}.instance{}", std::process::id());
                match conn.request_name_with_flags(fallback.as_str(), flags).await {
                    Ok(RequestNameReply::PrimaryOwner) => {
                        tracing::warn!(
                            "{BUS_NAME} is owned by another riptide; serving MPRIS as {fallback}"
                        );
                    }
                    other => {
                        tracing::warn!("MPRIS disabled: could not own a bus name: {other:?}");
                        return;
                    }
                }
            }
        }

        loop {
            if self.state_rx.changed().await.is_err() {
                break;
            }
            let new = self.state_rx.borrow_and_update().clone();
            let prev = std::mem::replace(&mut *shared.lock().unwrap(), new.clone());
            let plan = plan_signals(&prev, &new);
            if plan == SignalPlan::default() {
                continue;
            }

            let Ok(iface_ref) = conn
                .object_server()
                .interface::<_, PlayerIface>(OBJECT_PATH)
                .await
            else {
                continue;
            };
            let guard = iface_ref.get().await;
            let ctx = iface_ref.signal_context();
            if plan.status {
                let _ = guard.playback_status_changed(ctx).await;
            }
            if plan.metadata {
                let _ = guard.metadata_changed(ctx).await;
            }
            if plan.volume {
                let _ = guard.volume_changed(ctx).await;
            }
            if plan.shuffle {
                let _ = guard.shuffle_changed(ctx).await;
            }
            if plan.can_next {
                let _ = guard.can_go_next_changed(ctx).await;
            }
            if plan.can_prev {
                let _ = guard.can_go_previous_changed(ctx).await;
            }
            if plan.can_play_pause {
                let _ = guard.can_play_changed(ctx).await;
                let _ = guard.can_pause_changed(ctx).await;
            }
            if plan.can_seek {
                let _ = guard.can_seek_changed(ctx).await;
            }
            if plan.seeked {
                let _ = PlayerIface::seeked(ctx, new.position_us).await;
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn playing_state() -> MprisState {
        MprisState {
            track_id: 431291038,
            title: "Song".into(),
            artists: vec!["A".into(), "B".into()],
            album: "Album".into(),
            art_url: "https://resources.tidal.com/images/x/320x320.jpg".into(),
            duration_us: 180_000_000,
            position_us: 5_000_000,
            position_epoch: 0,
            paused: false,
            active: true,
            volume: 80,
            shuffle: false,
            can_next: true,
            can_prev: false,
            has_track: true,
        }
    }

    #[test]
    fn metadata_uses_per_track_id_path() {
        let map = build_metadata(&playing_state());
        let path = ObjectPath::try_from(map["mpris:trackid"].try_clone().unwrap()).unwrap();
        assert_eq!(path.as_str(), "/org/riptide/track/431291038");
        assert!(map.contains_key("xesam:title"));
        assert!(map.contains_key("xesam:artist"));
        assert!(map.contains_key("mpris:artUrl"));
        assert!(map.contains_key("mpris:length"));
    }

    #[test]
    fn metadata_without_track_is_the_no_track_sentinel() {
        let map = build_metadata(&MprisState::default());
        let path = ObjectPath::try_from(map["mpris:trackid"].try_clone().unwrap()).unwrap();
        assert_eq!(path.as_str(), NO_TRACK_PATH);
        assert!(!map.contains_key("xesam:title"));
        assert!(!map.contains_key("mpris:artUrl"));
    }

    #[test]
    fn set_position_track_path_round_trips() {
        assert_eq!(parse_track_path("/org/riptide/track/42"), Some(42));
        assert_eq!(parse_track_path(NO_TRACK_PATH), None);
        assert_eq!(parse_track_path("/org/riptide/track/abc"), None);
    }

    #[test]
    fn position_tick_alone_signals_nothing() {
        let prev = playing_state();
        let mut new = prev.clone();
        new.position_us += 500_000;
        assert_eq!(plan_signals(&prev, &new), SignalPlan::default());
    }

    #[test]
    fn pause_signals_only_status() {
        let prev = playing_state();
        let mut new = prev.clone();
        new.paused = true;
        let plan = plan_signals(&prev, &new);
        assert!(plan.status);
        assert!(!plan.metadata && !plan.seeked && !plan.volume);
    }

    #[test]
    fn track_change_signals_metadata() {
        let prev = playing_state();
        let mut new = prev.clone();
        new.track_id = 2;
        new.title = "Other".into();
        new.position_us = 0;
        let plan = plan_signals(&prev, &new);
        assert!(plan.metadata);
        assert!(!plan.status);
    }

    #[test]
    fn epoch_bump_signals_seeked() {
        let prev = playing_state();
        let mut new = prev.clone();
        new.position_us = 60_000_000;
        new.position_epoch += 1;
        let plan = plan_signals(&prev, &new);
        assert!(plan.seeked);
        assert!(!plan.metadata && !plan.status);
    }

    #[test]
    fn volume_and_shuffle_signal_their_properties() {
        let prev = playing_state();
        let mut new = prev.clone();
        new.volume = 50;
        new.shuffle = true;
        let plan = plan_signals(&prev, &new);
        assert!(plan.volume && plan.shuffle);
        assert!(!plan.metadata && !plan.status);
    }
}
