//! The window's tabs, as Windows Terminal's: each its own litastum in its
//! own pseudoconsole (`Session`), one shown at a time, with a bar of them
//! on top. Generic over the session so the bookkeeping is testable
//! without starting programs.

/// A tab's identity, carried by its terminal's events (`UserEvent`).
pub type TabId = u64;

/// Characters a tab title is cut to at most on the bar.
const MAX_TITLE: usize = 24;


pub struct Tab<S> {
    pub id: TabId,
    pub session: S,
    /// What the program last called itself (`OSC 0`/`2`), else its name.
    pub title: String,
    /// The screen litastum says it's on (`zoom::SCREEN_VARIABLE`), for
    /// that screen's zoom.
    pub screen: String,
}


/// The tabs, in bar order, and which one is shown.
pub struct Tabs<S> {
    tabs: Vec<Tab<S>>,
    active: usize,
    next_id: TabId,
}

impl<S> Tabs<S> {
    pub fn new() -> Self {
        Self { tabs: Vec::new(), active: 0, next_id: 1 }
    }

    /// An id for a tab about to be made -- its session needs it first.
    pub fn new_id(&mut self) -> TabId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Adds `tab` right after the shown one and shows it.
    pub fn add(&mut self, tab: Tab<S>) {
        let at = if self.tabs.is_empty() { 0 } else { self.active + 1 };
        self.tabs.insert(at, tab);
        self.active = at;
    }

    /// Drops the tab `id`; the shown one stays, or its right (else left)
    /// neighbor is shown. `false` once no tab is left.
    pub fn remove(&mut self, id: TabId) -> bool {
        if let Some(index) = self.index_of(id) {
            self.tabs.remove(index);
            if index < self.active || self.active >= self.tabs.len() {
                self.active = self.active.saturating_sub(1);
            }
        }
        !self.tabs.is_empty()
    }

    /// Shows the tab at `index`; whether that changed the shown one.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        true
    }

    /// `Ctrl+Tab`/`Ctrl+Shift+Tab`: the next or previous tab, round.
    pub fn step(&mut self, forward: bool) -> bool {
        let count = self.tabs.len();
        if count < 2 {
            return false;
        }
        let index = if forward { (self.active + 1) % count } else { (self.active + count - 1) % count };
        self.select(index)
    }

    pub fn active(&self) -> Option<&Tab<S>> {
        self.tabs.get(self.active)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn id_at(&self, index: usize) -> Option<TabId> {
        self.tabs.get(index).map(|tab| tab.id)
    }

    pub fn get(&self, id: TabId) -> Option<&Tab<S>> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab<S>> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Tab<S>> {
        self.tabs.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Tab<S>> {
        self.tabs.iter_mut()
    }

    fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }
}


/// What one cell of the bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarPart {
    /// Part of the tab at this index: its title and padding.
    Tab(usize),
    /// The tab's `×`: closes it.
    Close(usize),
    /// The `+` at the end: a new tab.
    New,
    Empty,
}


/// The bar `columns` cells wide: each tab as " title × ", then " + ",
/// titles cut evenly when they don't all fit.
pub fn bar(titles: &[&str], columns: usize) -> Vec<(char, BarPart)> {
    let mut cells = Vec::with_capacity(columns);
    let room = columns.saturating_sub(3);
    let per_tab = if titles.is_empty() { 0 } else { room / titles.len() };
    // " " + title + " × "
    let title_room = per_tab.saturating_sub(4).min(MAX_TITLE);
    for (index, title) in titles.iter().enumerate() {
        if title_room == 0 {
            break;
        }
        let shown: Vec<char> = title.chars().collect();
        let cut: String = if shown.len() > title_room { shown[..title_room - 1].iter().chain(std::iter::once(&'…')).collect() } else { shown.iter().collect() };
        cells.push((' ', BarPart::Tab(index)));
        cells.extend(cut.chars().map(|c| (c, BarPart::Tab(index))));
        cells.push((' ', BarPart::Tab(index)));
        cells.push(('×', BarPart::Close(index)));
        cells.push((' ', BarPart::Tab(index)));
    }
    cells.extend([(' ', BarPart::Empty), ('+', BarPart::New), (' ', BarPart::Empty)]);
    cells.truncate(columns);
    cells.resize(columns, (' ', BarPart::Empty));
    cells
}


#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(count: usize) -> Tabs<()> {
        let mut tabs = Tabs::new();
        for _ in 0..count {
            let id = tabs.new_id();
            tabs.add(Tab { id, session: (), title: format!("tab {id}"), screen: String::new() });
        }
        tabs
    }

    fn ids(tabs: &Tabs<()>) -> Vec<TabId> {
        tabs.iter().map(|tab| tab.id).collect()
    }

    #[test]
    fn a_new_tab_goes_after_the_shown_one_and_is_shown() {
        let mut tabs = tabs(3);
        tabs.select(0);
        let id = tabs.new_id();
        tabs.add(Tab { id, session: (), title: String::new(), screen: String::new() });

        assert_eq!(ids(&tabs), [1, 4, 2, 3]);
        assert_eq!(tabs.active().unwrap().id, 4);
    }

    /// Requested: Ctrl+Tab switches tabs, as in Windows Terminal.
    #[test]
    fn ctrl_tab_steps_round_both_ways() {
        let mut tabs = tabs(3);
        assert_eq!(tabs.active_index(), 2);

        assert!(tabs.step(true));
        assert_eq!(tabs.active_index(), 0, "past the last: the first");
        assert!(tabs.step(false));
        assert_eq!(tabs.active_index(), 2);
        let mut single = super::tests::tabs(1);
        assert!(!single.step(true), "one tab: nothing to switch to");
    }

    #[test]
    fn closing_keeps_the_shown_tab_or_shows_a_neighbor() {
        let mut tabs = tabs(4);
        tabs.select(1);

        assert!(tabs.remove(4));
        assert_eq!(tabs.active().unwrap().id, 2, "another tab closed: the shown one stays");
        assert!(tabs.remove(2));
        assert_eq!(tabs.active().unwrap().id, 3, "the shown one closed: its right neighbor");
        assert!(tabs.remove(3));
        assert_eq!(tabs.active().unwrap().id, 1, "no right one: the left");
        assert!(!tabs.remove(1), "none left");
    }

    #[test]
    fn the_bar_lays_out_titles_close_buttons_and_the_new_tab_button() {
        let cells = bar(&["rust", "docs"], 40);
        let text: String = cells.iter().map(|(c, _)| c).collect();

        assert_eq!(text.trim_end(), " rust ×  docs ×  +");
        let at = |column: usize| cells[column].1;
        assert_eq!(at(1), BarPart::Tab(0));
        assert_eq!(at(6), BarPart::Close(0));
        assert_eq!(at(9), BarPart::Tab(1));
        assert_eq!(at(17), BarPart::New);
        assert_eq!(at(30), BarPart::Empty);
    }

    #[test]
    fn long_titles_are_cut_to_fit() {
        let cells = bar(&["a-very-long-directory-name", "another-long-one"], 30);
        let text: String = cells.iter().map(|(c, _)| c).collect();

        assert_eq!(cells.len(), 30);
        assert!(text.contains('…') && text.contains('+'), "{text:?}");
    }
}
