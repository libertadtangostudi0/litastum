use super::{Entry, Panel};

/// Far Manager-style multi-select: `Shift+A` marks every real entry
/// (`select_all`); `Shift+Up`/`Down` toggles the entry under the cursor
/// and moves one row, so repeated presses paint a block one row at a
/// time (`toggle_mark_move_up`/`_down`); `Shift+Left`/`Right` does the
/// same but for the whole column(s) the existing paginated column jump
/// (`Panel::move_left`/`move_right`) crosses in one press
/// (`toggle_mark_move_left`/`_right`) -- requested directly as "should
/// mark the whole column(s) it jumps over," matching the row-at-a-time
/// behavior scaled up to the column-at-a-time unit `Left`/`Right`
/// already move by in this panel. Originally bound to `Ctrl` instead of
/// `Shift` throughout -- switched after a direct correction; see
/// `command_line::handle_browsing_key`'s own doc comment on the
/// `Shift+A` binding specifically for why it, alone among these, stays
/// gated to an empty command line even though the arrows aren't.
impl Panel {
    /// Whether `entries[index]` is currently marked -- read by
    /// `ui::build_list_item` to render it in the marked color. `false`
    /// for an out-of-range index rather than panicking, since the
    /// renderer already computes indices independently from the same
    /// `column_height`/`scroll_offset` math `Panel` itself uses.
    pub fn is_marked(&self, index: usize) -> bool {
        self.entries.get(index).is_some_and(|entry| self.marked.contains(&entry.name))
    }

    /// Every entry currently marked, in their listed (on-screen) order
    /// -- read by `marked_or_current` below.
    pub fn marked_entries(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|entry| self.marked.contains(&entry.name)).collect()
    }

    /// The entries a bulk panel operation (F5/F6/F8) should act on:
    /// every marked entry if any are marked, otherwise just the entry
    /// under the cursor (empty if that's `..`, or the panel has nothing
    /// in it) -- Far Manager-style, the marked set always wins over the
    /// cursor once anything's marked, regardless of where the cursor
    /// itself sits. Shared by `explorer::command::transfer_sources`
    /// (F5/F6) and `request_delete` (F8) so this rule lives in exactly
    /// one place rather than being reimplemented per command.
    pub fn marked_or_current(&self) -> Vec<&Entry> {
        let marked = self.marked_entries();
        if !marked.is_empty() {
            return marked;
        }
        self.current().filter(|entry| entry.name != "..").into_iter().collect()
    }

    /// Marks every entry except the synthetic `..` -- always marks
    /// outright rather than toggling all-marked back to none, matching
    /// the ordinary "select all" convention (Windows Explorer, VS Code,
    /// ...) rather than inventing a "select all" / "select none" toggle
    /// nobody asked for.
    pub fn select_all(&mut self) {
        self.marked = self.entries.iter().filter(|entry| entry.name != "..").map(|entry| entry.name.clone()).collect();
    }

    /// `Ctrl+Down`: toggles the mark on the current row, then moves
    /// down -- repeated presses paint (or un-paint, on an already-marked
    /// run) a contiguous block one row at a time.
    pub fn toggle_mark_move_down(&mut self) {
        self.toggle_mark_at(self.selected);
        self.move_down();
    }

    /// `Ctrl+Up`: mirror of `toggle_mark_move_down`.
    pub fn toggle_mark_move_up(&mut self) {
        self.toggle_mark_at(self.selected);
        self.move_up();
    }

    /// `Ctrl+Right`: toggles every row the paginated column jump
    /// (`move_right`) actually crosses -- from the row the cursor
    /// started on through the row it lands on, inclusive on both ends,
    /// not just the two endpoints.
    pub fn toggle_mark_move_right(&mut self) {
        let start = self.selected;
        self.move_right();
        self.toggle_mark_range(start, self.selected);
    }

    /// `Ctrl+Left`: mirror of `toggle_mark_move_right`.
    pub fn toggle_mark_move_left(&mut self) {
        let start = self.selected;
        self.move_left();
        self.toggle_mark_range(self.selected, start);
    }

    /// Toggles the mark on a single entry, skipping `..` -- there's
    /// nothing sensible to mark it *as*, and it should never end up
    /// counted as one of the "marked" entries a future multi-file
    /// operation would act on.
    fn toggle_mark_at(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        if entry.name == ".." {
            return;
        }
        let name = entry.name.clone();
        if !self.marked.remove(&name) {
            self.marked.insert(name);
        }
    }

    /// Toggles every entry in `[lo, hi]` (inclusive both ends) --
    /// `lo`/`hi` order doesn't matter, `toggle_mark_move_left`'s own
    /// start/end naturally arrive the "wrong" way round.
    fn toggle_mark_range(&mut self, lo: usize, hi: usize) {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        for index in lo..=hi {
            self.toggle_mark_at(index);
        }
    }
}
