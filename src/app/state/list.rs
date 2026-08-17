// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

//! Scrollable list state shared by every library view.

use super::*;

// ── StatefulList ──────────────────────────────────────────────────────────────

pub struct StatefulList<T> {
    pub items: Vec<T>,
    pub selected: usize,
    pub loading: bool,
    pub exhausted: bool,
    pub total: u32,
    pub pagination_cursor: Option<String>,
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

    #[allow(dead_code)]
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
            viewport: ListViewport::default(),
        }
    }
}

impl<T> StatefulList<T> {
    pub fn scroll_offset(&self, height: usize) -> usize {
        self.viewport
            .offset(self.selected, self.items.len(), height)
    }

    pub fn next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.items.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
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

    pub fn append(&mut self, new_items: Vec<T>, total: u32) {
        self.items.extend(new_items);
        self.total = total;
        self.exhausted = self.items.len() as u32 >= total;
        self.loading = false;
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
}
