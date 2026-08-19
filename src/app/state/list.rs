// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Scrollable list state shared by every library view.

use super::*;

// ── Filtering ─────────────────────────────────────────────────────────────────

/// An item a library list can be narrowed by as the user types.
pub trait Filterable {
    /// Text the filter query is matched against, lowercased.
    fn filter_text(&self) -> String;
}

impl Filterable for Track {
    fn filter_text(&self) -> String {
        format!("{} {}", self.title, self.all_artist_names()).to_lowercase()
    }
}

impl Filterable for Artist {
    fn filter_text(&self) -> String {
        self.name.to_lowercase()
    }
}

impl Filterable for Album {
    fn filter_text(&self) -> String {
        match &self.artist {
            Some(artist) => format!("{} {}", self.title, artist.name).to_lowercase(),
            None => self.title.to_lowercase(),
        }
    }
}

impl Filterable for Playlist {
    fn filter_text(&self) -> String {
        self.title.to_lowercase()
    }
}

// ── StatefulList ──────────────────────────────────────────────────────────────

pub struct StatefulList<T> {
    pub items: Vec<T>,
    pub selected: usize,
    pub loading: bool,
    pub exhausted: bool,
    pub total: u32,
    pub pagination_cursor: Option<String>,
    /// Live filter query as typed. Empty means "show everything", and that is
    /// a fast path: `matches` is never consulted, so lists that are never
    /// filtered cost nothing and a stale `matches` cannot be reached.
    filter: String,
    /// Indices into `items` that match `filter`. Only meaningful while `filter`
    /// is non-empty; keep it in step with `items` via `refilter`.
    matches: Vec<usize>,
    viewport: ListViewport,
}

#[derive(Default)]
pub(crate) struct ListViewport {
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

    pub(crate) fn reset(&self) {
        self.offset.set(0);
    }

    pub(crate) fn page_size(&self) -> usize {
        self.capacity.get().max(1)
    }

    pub(crate) fn previous_page(&self, selected: usize, len: usize) -> usize {
        if len == 0 {
            return selected;
        }
        let page_size = self.page_size();
        self.offset.set(self.offset.get().saturating_sub(page_size));
        selected.min(len - 1).saturating_sub(page_size)
    }

    pub(crate) fn next_page(&self, selected: usize, len: usize) -> usize {
        if len == 0 {
            return selected;
        }
        let page_size = self.page_size();
        self.offset.set(
            self.offset
                .get()
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
            total: 0,
            pagination_cursor: None,
            filter: String::new(),
            matches: Vec::new(),
            viewport: ListViewport::default(),
        }
    }
}

impl<T> StatefulList<T> {
    /// How many rows are on screen. `selected` indexes this sequence, not
    /// `items`, so every caller works the same whether or not a filter is on.
    pub fn visible_len(&self) -> usize {
        if self.filter.is_empty() {
            self.items.len()
        } else {
            self.matches.len()
        }
    }

    pub fn get_visible(&self, index: usize) -> Option<&T> {
        if self.filter.is_empty() {
            self.items.get(index)
        } else {
            self.matches.get(index).and_then(|&i| self.items.get(i))
        }
    }

    /// The visible rows in `[offset, offset + height)`, paired with their index
    /// in the visible sequence so renderers can compare against `selected`.
    pub fn visible_window(&self, height: usize) -> Vec<(usize, &T)> {
        let offset = self.scroll_offset(height);
        (offset..(offset + height).min(self.visible_len()))
            .filter_map(|i| self.get_visible(i).map(|item| (i, item)))
            .collect()
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn is_filtered(&self) -> bool {
        !self.filter.is_empty()
    }

    pub fn scroll_offset(&self, height: usize) -> usize {
        self.viewport
            .offset(self.selected, self.visible_len(), height)
    }

    pub fn next(&mut self) {
        let len = self.visible_len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(len - 1);
    }

    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn page_up(&mut self) {
        self.selected = self
            .viewport
            .previous_page(self.selected, self.visible_len());
    }

    pub fn page_down(&mut self) {
        self.selected = self.viewport.next_page(self.selected, self.visible_len());
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.get_visible(self.selected)
    }
}

impl<T: Filterable> StatefulList<T> {
    pub fn append(&mut self, new_items: Vec<T>, total: u32) {
        self.items.extend(new_items);
        self.total = total;
        self.exhausted = self.items.len() as u32 >= total;
        self.loading = false;
        self.refilter();
    }

    /// Append a page from a cursor-paginated collection.
    ///
    /// The v2 collection endpoints report no total at all — the response carries
    /// `data`, `included` and `links` only — so the cursor is the one thing that
    /// can say whether more pages exist, and `total` can only ever mean "what has
    /// arrived so far". Deriving `exhausted` from a total instead stops paging
    /// after the first page, because a page-sized total always satisfies it.
    pub fn append_page(&mut self, new_items: Vec<T>, next_cursor: Option<String>) {
        self.items.extend(new_items);
        self.total = self.items.len() as u32;
        self.exhausted = next_cursor.is_none();
        self.pagination_cursor = next_cursor;
        self.loading = false;
        self.refilter();
    }

    /// Edit the query and re-narrow. Selection returns to the top because the
    /// row it pointed at is unlikely to still be there.
    pub fn edit_filter(&mut self, edit: impl FnOnce(&mut String)) {
        edit(&mut self.filter);
        self.selected = 0;
        self.viewport.reset();
        self.refilter();
    }

    /// Drop the items `keep` rejects, holding `total`, the filter and the
    /// selection consistent with what is left.
    pub fn remove_where(&mut self, keep: impl FnMut(&T) -> bool) {
        let before = self.items.len();
        self.items.retain(keep);
        let removed = (before - self.items.len()) as u32;
        self.total = self.total.saturating_sub(removed);
        self.refilter();
        self.selected = self.selected.min(self.visible_len().saturating_sub(1));
    }

    /// Recompute `matches`. Call after anything mutates `items` directly, or the
    /// indices go stale. Free when no filter is set.
    pub fn refilter(&mut self) {
        if self.filter.is_empty() {
            self.matches.clear();
            return;
        }
        let needle = self.filter.to_lowercase();
        self.matches = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.filter_text().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Lets the list tests stay on plain numbers. Filtering by digit is enough to
    /// exercise the index mapping without dragging in a full `Track`.
    impl Filterable for u32 {
        fn filter_text(&self) -> String {
            self.to_string()
        }
    }

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
        assert_eq!(list.total, 5);
        assert!(!list.exhausted);
        assert!(!list.loading);
    }

    #[test]
    fn stateful_list_append_marks_exhausted_on_last_page() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![1, 2, 3], 3);
        assert!(list.exhausted);
    }

    #[test]
    fn stateful_list_append_accumulates_pages() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append(vec![1, 2], 4);
        assert!(!list.exhausted);
        list.append(vec![3, 4], 4);
        assert_eq!(list.items, vec![1, 2, 3, 4]);
        assert!(list.exhausted);
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

    // ── Filtering ─────────────────────────────────────────────────────────────

    fn filtered(items: Vec<u32>, query: &str) -> StatefulList<u32> {
        let mut list: StatefulList<u32> = StatefulList::default();
        let total = items.len() as u32;
        list.append(items, total);
        list.edit_filter(|f| f.push_str(query));
        list
    }

    #[test]
    fn filtering_narrows_the_visible_rows() {
        let list = filtered((1..=25).collect(), "2");

        // 2, 12, 20..25 — everything containing the digit.
        assert_eq!(list.visible_len(), 8);
        assert_eq!(list.items.len(), 25, "the full list is kept intact");
    }

    #[test]
    fn selection_addresses_the_visible_rows_not_the_backing_list() {
        let mut list = filtered((1..=25).collect(), "2");

        assert_eq!(list.selected_item(), Some(&2));
        list.next();
        assert_eq!(list.selected_item(), Some(&12));
        list.next();
        assert_eq!(list.selected_item(), Some(&20));
    }

    #[test]
    fn navigation_stays_inside_the_filtered_set() {
        let mut list = filtered((1..=25).collect(), "2");

        for _ in 0..50 {
            list.next();
        }
        assert_eq!(list.selected, list.visible_len() - 1);
        assert_eq!(list.selected_item(), Some(&25));

        list.page_down();
        assert!(list.selected < list.visible_len());
    }

    #[test]
    fn narrowing_the_filter_clamps_a_selection_that_falls_off_the_end() {
        let mut list = filtered((1..=25).collect(), "2");
        list.selected = list.visible_len() - 1;

        // "23" matches a single row, well short of the old selection.
        list.edit_filter(|f| {
            f.clear();
            f.push_str("23")
        });

        assert_eq!(list.visible_len(), 1);
        assert_eq!(list.selected_item(), Some(&23));
    }

    #[test]
    fn clearing_the_filter_restores_the_whole_list() {
        let mut list = filtered((1..=25).collect(), "23");
        list.edit_filter(|f| f.clear());

        assert!(!list.is_filtered());
        assert_eq!(list.visible_len(), 25);
        assert_eq!(list.selected_item(), Some(&1));
    }

    #[test]
    fn pages_arriving_during_an_active_filter_are_matched_too() {
        let mut list = filtered((1..=9).collect(), "2");
        assert_eq!(list.visible_len(), 1);

        list.append((20..=22).collect(), 12);

        assert_eq!(list.visible_len(), 4);
        assert_eq!(list.selected_item(), Some(&2));
    }

    #[test]
    fn removing_an_item_keeps_the_filtered_view_consistent() {
        let mut list = filtered((1..=25).collect(), "2");
        let before = list.visible_len();

        list.remove_where(|n| *n != 12);

        assert_eq!(list.visible_len(), before - 1);
        assert_eq!(list.total, 24);
        assert!(list.selected < list.visible_len());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let mut list: StatefulList<String> = StatefulList::default();
        list.append(
            vec!["Tsunami Sea".to_string(), "Circle With Me".to_string()],
            2,
        );
        list.edit_filter(|f| f.push_str("TSUNAMI"));

        assert_eq!(list.visible_len(), 1);
    }

    impl Filterable for String {
        fn filter_text(&self) -> String {
            self.to_lowercase()
        }
    }

    // ── Cursor pagination ─────────────────────────────────────────────────────

    #[test]
    fn a_page_sized_batch_does_not_end_pagination() {
        // The regression: these endpoints report no total, so a total was
        // synthesised from the page length and `items.len() >= total` marked the
        // list finished after page one — capping Albums at 20.
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append_page((0..20).collect(), Some("cursor-1".to_string()));

        assert!(!list.exhausted, "a full page means more may follow");
        assert_eq!(list.pagination_cursor.as_deref(), Some("cursor-1"));
    }

    #[test]
    fn only_a_missing_cursor_ends_pagination() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append_page((0..20).collect(), Some("c".to_string()));
        list.append_page((20..40).collect(), None);

        assert!(list.exhausted);
        assert!(list.pagination_cursor.is_none());
        assert_eq!(list.items.len(), 40);
    }

    #[test]
    fn total_counts_what_has_arrived_across_pages() {
        let mut list: StatefulList<u32> = StatefulList::default();
        list.append_page((0..20).collect(), Some("c".to_string()));
        assert_eq!(list.total, 20);
        list.append_page((20..50).collect(), None);
        assert_eq!(list.total, 50, "the header must not report one page");
    }

    #[test]
    fn a_track_matches_on_its_artist_as_well_as_its_title() {
        let track = Track {
            id: 1,
            title: "Circle With Me".to_string(),
            duration: 180,
            artist: Some(ArtistRef {
                name: "Spiritbox".to_string(),
            }),
            artists: vec![],
            album: Album {
                id: 1,
                title: "Eternal Blue".to_string(),
                number_of_tracks: None,
                release_date: None,
                cover: None,
                artist: None,
                media_metadata: None,
                added_at: None,
                album_type: None,
            },
            media_metadata: None,
            added_at: None,
        };

        let text = track.filter_text();
        assert!(text.contains("circle"), "matches the title");
        assert!(text.contains("spiritbox"), "matches the artist");
        assert!(
            !text.contains("eternal"),
            "the album is deliberately not matched"
        );
    }
}
