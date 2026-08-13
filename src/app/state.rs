// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use crate::api::models::*;
use crate::playlist::PlaylistDetail;
use std::cell::Cell;

// ── Tab ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Favorites,
    Artists,
    Albums,
    Playlists,
    Search,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Home, Tab::Favorites, Tab::Artists, Tab::Albums, Tab::Playlists, Tab::Search];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Home      => "Home",
            Tab::Favorites => "Tracks",
            Tab::Artists   => "Artists",
            Tab::Albums    => "Albums",
            Tab::Playlists => "Playlists",
            Tab::Search    => "Search",
        }
    }
}

// ── StatefulList ──────────────────────────────────────────────────────────────

pub struct StatefulList<T> {
    pub items: Vec<T>,
    pub selected: usize,
    pub loading: bool,
    pub exhausted: bool,
    pub next_offset: u32,
    pub total: u32,
    pub last_load_triggered_at: usize,
    pub pagination_cursor: Option<String>,
    viewport: ListViewport,
}

#[derive(Default)]
pub(super) struct ListViewport {
    offset: Cell<usize>,
    capacity: Cell<usize>,
}

impl ListViewport {
    /// Keep `selected` visible while preserving the current window until the
    /// cursor crosses one of its edges.
    pub fn offset(&self, selected: usize, len: usize, height: usize) -> usize {
        self.capacity.set(height);
        if height == 0 || len <= height {
            self.offset.set(0);
            return 0;
        }

        let selected = selected.min(len - 1);
        let mut offset = self.offset.get().min(len - height);
        if selected < offset {
            offset = selected;
        } else if selected >= offset + height {
            offset = selected + 1 - height;
        }
        offset = offset.min(len - height);
        self.offset.set(offset);
        offset
    }

    #[allow(dead_code)]
    pub(super) fn reset(&self) {
        self.offset.set(0);
    }

    pub(super) fn page_size(&self) -> usize {
        self.capacity.get().max(1)
    }

    pub(super) fn previous_page(&self, selected: usize, len: usize) -> usize {
        if len == 0 { return selected; }
        let page_size = self.page_size();
        self.offset.set(self.offset.get().saturating_sub(page_size));
        selected.min(len - 1).saturating_sub(page_size)
    }

    pub(super) fn next_page(&self, selected: usize, len: usize) -> usize {
        if len == 0 { return selected; }
        let page_size = self.page_size();
        self.offset.set(
            self.offset.get()
                .saturating_add(page_size)
                .min(len.saturating_sub(page_size)),
        );
        selected.saturating_add(page_size).min(len - 1)
    }
}

impl<T> Default for StatefulList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            loading: false,
            exhausted: false,
            next_offset: 0,
            total: 0,
            last_load_triggered_at: 0,
            pagination_cursor: None,
            viewport: ListViewport::default(),
        }
    }
}

impl<T> StatefulList<T> {
    pub fn scroll_offset(&self, height: usize) -> usize {
        self.viewport.offset(self.selected, self.items.len(), height)
    }

    pub fn next(&mut self) {
        if self.items.is_empty() { return; }
        self.selected = (self.selected + 1).min(self.items.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.selected > 0 { self.selected -= 1; }
    }

    pub fn page_up(&mut self) {
        self.selected = self.viewport.previous_page(self.selected, self.items.len());
    }

    pub fn page_down(&mut self) {
        self.selected = self.viewport.next_page(self.selected, self.items.len());
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.items.get(self.selected)
    }

    pub fn should_load_more(&self) -> bool {
        !self.loading
            && !self.exhausted
            && !self.items.is_empty()
            && self.selected >= self.items.len().saturating_sub(10)
            && self.items.len() > self.last_load_triggered_at
    }

    pub fn append(&mut self, new_items: Vec<T>, total: u32) {
        self.next_offset = (self.items.len() + new_items.len()) as u32;
        self.total = total;
        self.exhausted = self.next_offset >= total;
        self.items.extend(new_items);
        self.loading = false;
    }
}

// ── Artist detail ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistDetailFocus {
    Tracks,
    Albums,
    EPs,
    Singles,
    Bio,
}

pub struct ArtistDetail {
    pub artist: Artist,
    pub tracks: StatefulList<Track>,
    pub albums: StatefulList<Album>,
    pub eps: StatefulList<Album>,
    pub singles: StatefulList<Album>,
    pub focus: ArtistDetailFocus,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub bio: Option<String>,
    pub bio_loading: bool,
    pub bio_scroll: u16,
}

// ── Playlist detail ───────────────────────────────────────────────────────────

// ── Home tab ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSectionFocus {
    NewReleases,
    DailyMixes,
    DiscoveryMixes,
}

impl Default for HomeSectionFocus {
    fn default() -> Self {
        Self::NewReleases
    }
}

pub struct HomeSection<T> {
    pub items: Vec<T>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T> Default for HomeSection<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            loading: false,
            error: None,
        }
    }
}

impl<T> HomeSection<T> {
    pub fn next(&mut self) {
        if self.items.is_empty() { return; }
        self.selected = (self.selected + 1).min(self.items.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.selected > 0 { self.selected -= 1; }
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.items.get(self.selected)
    }
}

// ── Album detail / art payload ────────────────────────────────────────────────


pub struct AlbumDetail {
    pub album: Album,
    pub tracks: StatefulList<Track>,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
}

// ── View stack ────────────────────────────────────────────────────────────────

pub enum View {
    ArtistDetail(ArtistDetail),
    PlaylistDetail(PlaylistDetail),
    AlbumDetail(AlbumDetail),
}


// ── Sort palette ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    Alphabetical,
    LastAdded,
    ByArtist,
}

pub struct SortPalette {
    pub active: bool,
    pub selected: usize,
}

impl Default for SortPalette {
    fn default() -> Self {
        Self { active: false, selected: 0 }
    }
}

impl SortPalette {
    pub fn get_options(current_tab: Tab) -> &'static [(&'static str, SortField)] {
        match current_tab {
            Tab::Home => &[],
            Tab::Artists | Tab::Playlists => &[
                ("Alphabetical", SortField::Alphabetical),
                ("Last Added",   SortField::LastAdded)
            ],
            Tab::Albums | Tab::Favorites => &[
                ("Alphabetical", SortField::Alphabetical),
                ("By Artist",    SortField::ByArtist),
                ("Last Added",   SortField::LastAdded)
            ],
            Tab::Search => &[],
        }
    }
}

// ── Artist selection modal ────────────────────────────────────────────────────

pub struct ArtistSelection {
    pub active: bool,
    pub artist_names: Vec<String>,
    pub selected: usize,
    pub searching_for: Option<String>,
}

impl Default for ArtistSelection {
    fn default() -> Self {
        Self { active: false, artist_names: Vec::new(), selected: 0, searching_for: None }
    }
}

// ── Command palette ───────────────────────────────────────────────────────────

pub struct CommandState {
    pub active: bool,
    pub input: String,
    pub selected: usize,
}

impl Default for CommandState {
    fn default() -> Self {
        Self { active: false, input: String::new(), selected: 0 }
    }
}

impl CommandState {
    pub const COMMANDS: &'static [&'static str] =
        &["home", "favorites", "artists", "albums", "playlists", "search"];

    pub fn matches(&self) -> Vec<&'static str> {
        let q = self.input.to_lowercase();
        Self::COMMANDS.iter()
            .filter(|&&c| c.starts_with(q.as_str()))
            .copied()
            .collect()
    }
}

// ── Now playing ───────────────────────────────────────────────────────────────

pub struct NowPlaying {
    pub track: Option<Track>,
    /// True only after mpv fires TrackStarted; false on startup and after the queue empties.
    pub active: bool,
    pub paused: bool,
    pub position: f64,
    pub duration: f64,
    pub queue: Vec<Track>,
    pub queue_index: usize,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub lyrics_synced: Vec<(f64, String)>,
    pub lyrics_plain: Vec<String>,
    pub lyrics_loading: bool,
    pub sample_rate: Option<u32>,
    pub codec: Option<String>,
    pub volume: u8,
    pub shuffle: bool,
    /// UUID of the playlist this queue originated from, used to append arriving pages.
    pub source_playlist_uuid: Option<String>,
    /// How many tracks from that playlist have been loaded into the queue so far.
    pub source_playlist_next_offset: u32,
    /// Cursor for pagination of the source playlist
    pub source_playlist_cursor: Option<String>,
    /// Saved queue order before shuffling; restored when shuffle is toggled off.
    pub original_queue: Vec<Track>,
    /// Whether this track has been sent to Last.fm for scrobbling
    pub lastfm_sent: bool,
}

impl Default for NowPlaying {
    fn default() -> Self {
        Self {
            track: None,
            active: false,
            paused: true,
            position: 0.0,
            duration: 0.0,
            queue: Vec::new(),
            queue_index: 0,
            art_bytes: None,
            art_loading: false,
            lyrics_synced: Vec::new(),
            lyrics_plain: Vec::new(),
            lyrics_loading: false,
            sample_rate: None,
            codec: None,
            volume: 100,
            shuffle: false,
            source_playlist_uuid: None,
            source_playlist_next_offset: 0,
            source_playlist_cursor: None,
            original_queue: Vec::new(),
            lastfm_sent: false,
        }
    }
}

impl NowPlaying {
    pub fn progress_ratio(&self) -> f64 {
        if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn position_display(&self) -> String {
        fmt_secs(self.position as u32)
    }

    pub fn duration_display(&self) -> String {
        fmt_secs(self.duration as u32)
    }
}

pub(super) fn fmt_secs(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

// ── Status level ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Error,
}

// ── Keybinds ──────────────────────────────────────────────────────────────────

pub struct Keybind {
    pub key: &'static str,
    pub action: &'static str,
}

pub struct KeybindGroup {
    pub title: &'static str,
    pub binds: &'static [Keybind],
}

impl KeybindGroup {
    /// Calculate total lines needed to display all keybind groups (for scrolling bounds)
    pub fn total_help_lines() -> u16 {
        let groups = vec![
            Self::global(),
            Self::navigation(),
            Self::queue(),
            Self::search(),
            Self::command(),
        ];
        let mut total = 0u16;
        for group in groups {
            total += 1; // group header
            total += group.binds.len() as u16; // keybinds
            total += 1; // blank line
        }
        total
    }

    pub fn global() -> Self {
        KeybindGroup {
            title: "Global",
            binds: &[
                Keybind { key: "?", action: "Show this help" },
                Keybind { key: "q", action: "Quit" },
                Keybind { key: "/", action: "Command palette" },
                Keybind { key: "Tab", action: "Next tab" },
                Keybind { key: "Shift+Tab", action: "Previous tab" },
                Keybind { key: "Space", action: "Play/Pause" },
                Keybind { key: "n", action: "Next track" },
                Keybind { key: "p", action: "Previous track" },
                Keybind { key: "z", action: "Toggle shuffle" },
                Keybind { key: "+ or =", action: "Volume Up"},
                Keybind { key: "-", action: "Volume Down"},
                Keybind { key: "Esc", action: "Back/Go up" },
            ],
        }
    }

    pub fn navigation() -> Self {
        KeybindGroup {
            title: "Navigation",
            binds: &[
                Keybind { key: "↑", action: "Up" },
                Keybind { key: "↓", action: "Down" },
                Keybind { key: "PgUp/PgDn", action: "Move one page" },
                Keybind { key: "Enter", action: "Select/Open" },
                Keybind { key: "a", action: "Add to queue" },
                Keybind { key: "f", action: "Toggle favorite/follow/save" },
                Keybind { key: "g", action: "Go to artist" },
                Keybind { key: "s", action: "Sort" },
                Keybind { key: "r", action: "Start radio" },
                Keybind { key: "c", action: "Copy share link (song)" },
                Keybind { key: "C", action: "Copy share link (album/playlist)" },
                Keybind { key: "→", action: "Focus queue" },
            ],
        }
    }

    pub fn queue() -> Self {
        KeybindGroup {
            title: "Queue",
            binds: &[
                Keybind { key: "↑", action: "Up" },
                Keybind { key: "↓", action: "Down" },
                Keybind { key: "d", action: "Remove track" },
                Keybind { key: "c", action: "Copy share link (song)" },
                Keybind { key: "C", action: "Copy share link (album)" },
                Keybind { key: "Enter", action: "Play track" },
                Keybind { key: "Esc", action: "Close queue" },
            ],
        }
    }

    pub fn search() -> Self {
        KeybindGroup {
            title: "Search",
            binds: &[
                Keybind { key: "↑", action: "Up" },
                Keybind { key: "↓", action: "Down" },
                Keybind { key: "Tab", action: "Next pane" },
                Keybind { key: "Shift+Tab", action: "Prev pane" },
                Keybind { key: "Enter", action: "Select/Open" },
                Keybind { key: "Esc", action: "Close search" },
            ],
        }
    }

    pub fn command() -> Self {
        KeybindGroup {
            title: "Command",
            binds: &[
                Keybind { key: "↑", action: "Up" },
                Keybind { key: "↓", action: "Down" },
                Keybind { key: "Enter", action: "Execute" },
                Keybind { key: "Esc", action: "Close" },
            ],
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── fmt_secs ──────────────────────────────────────────────────────────────

    #[test]
    fn fmt_secs_zero() {
        assert_eq!(fmt_secs(0), "0:00");
    }

    #[test]
    fn fmt_secs_sub_minute() {
        assert_eq!(fmt_secs(59), "0:59");
    }

    #[test]
    fn fmt_secs_exact_minute() {
        assert_eq!(fmt_secs(60), "1:00");
    }

    #[test]
    fn fmt_secs_minutes_and_seconds() {
        assert_eq!(fmt_secs(90), "1:30");
        assert_eq!(fmt_secs(3661), "61:01");
    }

    // ── StatefulList ──────────────────────────────────────────────────────────

    #[test]
    fn stateful_list_append_updates_state() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![1, 2, 3], 5);
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.next_offset, 3);
        assert_eq!(list.total, 5);
        assert!(!list.exhausted);
        assert!(!list.loading);
    }

    #[test]
    fn stateful_list_append_marks_exhausted_on_last_page() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![1, 2, 3], 3);
        assert!(list.exhausted);
        assert_eq!(list.next_offset, 3);
    }

    #[test]
    fn stateful_list_append_accumulates_pages() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![1, 2], 4);
        assert!(!list.exhausted);
        list.append(vec![3, 4], 4);
        assert_eq!(list.items, vec![1, 2, 3, 4]);
        assert!(list.exhausted);
        assert_eq!(list.next_offset, 4);
    }

    #[test]
    fn stateful_list_next_stays_in_bounds() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![10, 20, 30], 3);
        list.next();
        assert_eq!(list.selected, 1);
        list.next();
        list.next(); // already at last item
        assert_eq!(list.selected, 2);
    }

    #[test]
    fn stateful_list_prev_stays_in_bounds() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![10, 20, 30], 3);
        list.selected = 2;
        list.prev();
        assert_eq!(list.selected, 1);
        list.prev();
        list.prev(); // already at first item
        assert_eq!(list.selected, 0);
    }

    #[test]
    fn stateful_list_next_on_empty_is_no_op() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.next();
        assert_eq!(list.selected, 0);
    }

    #[test]
    fn scroll_offset_page_moves_only_at_edges() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..20u32).collect(), 20);

        // Cursor moves within the window without scrolling.
        for sel in 0..5 {
            list.selected = sel;
            assert_eq!(list.scroll_offset(5), 0);
        }
        // Crossing the bottom edge shifts the page.
        list.selected = 5;
        assert_eq!(list.scroll_offset(5), 1);
        list.selected = 10;
        assert_eq!(list.scroll_offset(5), 6);

        // Moving back up, the cursor climbs within the window first…
        for sel in (6..10).rev() {
            list.selected = sel;
            assert_eq!(list.scroll_offset(5), 6);
        }
        // …and only crossing the top edge scrolls the page.
        list.selected = 5;
        assert_eq!(list.scroll_offset(5), 5);
    }

    #[test]
    fn scroll_offset_clamps_when_list_shrinks() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..20u32).collect(), 20);
        list.selected = 19;
        assert_eq!(list.scroll_offset(5), 15);

        list.items.truncate(8);
        list.selected = 7;
        assert_eq!(list.scroll_offset(5), 3);

        list.items.truncate(4); // fits entirely in the window
        list.selected = 3;
        assert_eq!(list.scroll_offset(5), 0);
    }

    #[test]
    fn scroll_offset_handles_stale_selection() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..20u32).collect(), 20);
        list.selected = 100;

        assert_eq!(list.scroll_offset(5), 15);
    }

    #[test]
    fn list_viewport_can_be_reset() {
        let viewport = ListViewport::default();
        assert_eq!(viewport.offset(10, 20, 5), 6);

        viewport.reset();
        assert_eq!(viewport.offset(0, 20, 5), 0);
    }

    #[test]
    fn stateful_list_pages_by_viewport_height() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..20u32).collect(), 20);
        assert_eq!(list.scroll_offset(5), 0);

        list.page_down();
        assert_eq!(list.selected, 5);
        assert_eq!(list.scroll_offset(5), 5);

        list.page_down();
        assert_eq!(list.selected, 10);
        assert_eq!(list.scroll_offset(5), 10);

        list.page_up();
        assert_eq!(list.selected, 5);
        assert_eq!(list.scroll_offset(5), 5);
    }

    #[test]
    fn stateful_list_paging_clamps_at_boundaries() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..12u32).collect(), 12);
        assert_eq!(list.scroll_offset(5), 0);

        list.page_up();
        assert_eq!(list.selected, 0);

        list.selected = 10;
        list.page_down();
        assert_eq!(list.selected, 11);

        let mut empty: StatefulList<u32> = StatefulList::default();
        empty.page_up();
        empty.page_down();
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn stateful_list_should_load_more_triggers_near_end() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..20u32).collect(), 100);
        // triggers when selected >= items.len() - 10 → at selected == 10
        list.selected = 10;
        assert!(list.should_load_more());
        list.selected = 9;
        assert!(!list.should_load_more());
    }

    #[test]
    fn stateful_list_should_load_more_false_when_exhausted() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..5u32).collect(), 5);
        list.selected = 4;
        assert!(!list.should_load_more()); // exhausted
    }

    #[test]
    fn stateful_list_should_load_more_false_while_loading() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append((0..20u32).collect(), 100);
        list.selected = 15;
        list.loading = true;
        assert!(!list.should_load_more());
    }

    // ── CommandState ──────────────────────────────────────────────────────────

    #[test]
    fn command_state_matches_prefix() {
        let mut cmd = CommandState::default();
        cmd.input = "fav".to_string();
        let matches = cmd.matches();
        assert!(matches.contains(&"favorites"));
        assert!(!matches.contains(&"artists"));
    }

    #[test]
    fn command_state_empty_input_matches_all() {
        let cmd = CommandState::default();
        let matches = cmd.matches();
        assert_eq!(matches.len(), CommandState::COMMANDS.len());
    }

    #[test]
    fn command_state_no_match_returns_empty() {
        let mut cmd = CommandState::default();
        cmd.input = "zzz".to_string();
        assert!(cmd.matches().is_empty());
    }
}
