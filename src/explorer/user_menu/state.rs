use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use super::parse::{self, MenuItem, MenuItemBody, Prompt};

/// litastum's own user-menu file name -- read/written directly, no
/// migration concerns once it exists.
const OWN_FILE_NAME: &str = "LitastumMenu.ini";
/// Real Far Manager's own per-directory user-menu file name.
const FAR_FILE_NAME: &str = "FarMenu.ini";

/// Finds `dir`'s user-menu file, preferring litastum's own
/// `LitastumMenu.ini`; falls back to a real `FarMenu.ini` if that's all
/// that's there, copying its content into `LitastumMenu.ini` right
/// away so every future `F2` in this directory reads litastum's own
/// file from then on -- `FarMenu.ini` itself is never modified or
/// deleted, only read once as the seed for our own copy. If the copy
/// itself fails (read-only directory, permissions, ...) this still
/// returns `FarMenu.ini` so the menu works for this session regardless
/// -- best-effort persistence, same "never block on a failed write"
/// rule `theming::config` already follows for theme/setup persistence.
fn resolve_menu_file(dir: &Path) -> Option<PathBuf> {
    let own = dir.join(OWN_FILE_NAME);
    if own.is_file() {
        return Some(own);
    }

    let far = dir.join(FAR_FILE_NAME);
    if !far.is_file() {
        return None;
    }

    match fs::read_to_string(&far) {
        Ok(content) => match fs::write(&own, &content) {
            Ok(()) => {
                debug!(from = %far.display(), to = %own.display(), "migrated FarMenu.ini into LitastumMenu.ini");
                Some(own)
            }
            Err(err) => {
                warn!(path = %own.display(), %err, "failed to migrate FarMenu.ini into LitastumMenu.ini; using FarMenu.ini directly this session");
                Some(far)
            }
        },
        Err(err) => {
            warn!(path = %far.display(), %err, "FarMenu.ini exists but could not be read");
            None
        }
    }
}


/// Loads and parses `dir`'s user menu, if it has one at all (see
/// `resolve_menu_file`). `None` only when there's no menu file to read
/// -- a present-but-empty or malformed one still returns `Some(vec![])`
/// (`parse::parse` never fails outright, see its own doc comment).
pub fn load_menu(dir: &Path) -> Option<Vec<MenuItem>> {
    let path = resolve_menu_file(dir)?;
    match fs::read_to_string(&path) {
        Ok(content) => Some(parse::parse(&content)),
        Err(err) => {
            warn!(path = %path.display(), %err, "user menu file could not be read");
            None
        }
    }
}


/// Creates an empty `LitastumMenu.ini` in `dir` -- `F2` calls this when
/// `resolve_menu_file` finds neither it nor a `FarMenu.ini` to migrate,
/// so there's actually something to open in the built-in editor right
/// away (`explorer::command::open_user_menu`) instead of browsing an
/// empty popup with nothing in it to select. `None` if the write itself
/// fails (a read-only directory, permissions, ...) -- `F2` just does
/// nothing then, same as any other "couldn't act on this" case in this
/// codebase.
pub fn create_menu_file(dir: &Path) -> Option<PathBuf> {
    let path = dir.join(OWN_FILE_NAME);
    fs::write(&path, "").ok()?;
    Some(path)
}


/// One level of `UserMenuState`'s own navigation stack -- the items
/// visible at that level, and which one the cursor is on.
pub struct UserMenuLevel {
    pub items: Vec<MenuItem>,
    pub selected: usize,
}


/// `F2`: browsing a (possibly nested) user menu. `stack.last()` is the
/// level currently shown; entering a submenu pushes a new level on top
/// without discarding the one below (so `Esc` restores it exactly,
/// cursor position included), same shape as `theming::MainMenu`'s own
/// two-level `Main`/`Commands`/`Options` navigation, just generalized
/// to arbitrary depth since a real menu file can nest as deep as its
/// author likes.
pub struct UserMenuState {
    stack: Vec<UserMenuLevel>,
}

impl UserMenuState {
    /// `None` if `dir` has neither a `LitastumMenu.ini` nor a
    /// `FarMenu.ini` to read at all -- `explorer::command::open_user_menu`
    /// creates an empty `LitastumMenu.ini` and opens it for editing
    /// instead in that case, rather than browsing an empty popup with
    /// nothing in it to select.
    pub fn open(dir: &Path) -> Option<Self> {
        let items = load_menu(dir)?;
        Some(Self { stack: vec![UserMenuLevel { items, selected: 0 }] })
    }

    pub fn current_level(&self) -> &UserMenuLevel {
        self.stack.last().expect("stack is never empty -- open() always seeds one level, and back() refuses to pop the last one")
    }

    fn current_level_mut(&mut self) -> &mut UserMenuLevel {
        self.stack.last_mut().expect("see current_level's own comment")
    }

    pub fn move_up(&mut self) {
        let level = self.current_level_mut();
        level.selected = level.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let level = self.current_level_mut();
        if level.selected + 1 < level.items.len() {
            level.selected += 1;
        }
    }

    pub fn selected_item(&self) -> Option<&MenuItem> {
        let level = self.current_level();
        level.items.get(level.selected)
    }

    /// Descends into the highlighted item if it's a submenu -- `true`
    /// on success. A no-op (`false`) for a `Commands` item or an empty
    /// level; the caller is expected to try running it as commands
    /// instead in that case.
    pub fn enter_submenu(&mut self) -> bool {
        let Some(MenuItem { body: MenuItemBody::Submenu(children), .. }) = self.selected_item() else {
            return false;
        };
        let children = children.clone();
        self.stack.push(UserMenuLevel { items: children, selected: 0 });
        true
    }

    /// Backs up one level. `true` if it moved up a level (the caller
    /// stays in the menu); `false` if already at the top level (the
    /// caller should close the menu entirely) -- same contract as
    /// `theming::MainMenu::back`.
    pub fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }
}


/// `Mode::UserMenuPrompt`: collecting answers to a `Commands` item's
/// own `!?Label?Default!` placeholders (`parse::extract_prompts`)
/// before running it -- one text field shown at a time, in
/// first-appearance order, same as real Far Manager's own "each
/// distinct label prompts once" behavior (`parse::extract_prompts`'s
/// own doc comment).
pub struct UserMenuPromptState {
    /// The item's own commands, with `!&` already substituted --
    /// `!?Label?Default!` placeholders are still present until
    /// `accept_current` finishes collecting every answer.
    commands: Vec<String>,
    prompts: Vec<Prompt>,
    current: usize,
    answers: Vec<(String, String)>,
    pub value: String,
    pub cursor: usize,
    pub selection_anchor: Option<usize>,
}

impl UserMenuPromptState {
    /// `prompts` must be non-empty -- callers only build this once
    /// `parse::extract_prompts` on `commands` actually found at least
    /// one placeholder; a `Commands` item with none just runs directly
    /// instead (`explorer::user_menu::input`).
    pub fn new(commands: Vec<String>, prompts: Vec<Prompt>) -> Self {
        assert!(!prompts.is_empty(), "UserMenuPromptState needs at least one prompt to collect");
        let value = prompts[0].default.clone();
        let cursor = value.chars().count();
        Self { commands, prompts, current: 0, answers: Vec::new(), value, cursor, selection_anchor: None }
    }

    /// The label to show above the input field for whichever prompt is
    /// currently being asked.
    pub fn current_label(&self) -> &str {
        &self.prompts[self.current].label
    }

    /// How many prompts remain including this one, and the total --
    /// e.g. "2 of 3", for the popup's own title/footer.
    pub fn progress(&self) -> (usize, usize) {
        (self.current + 1, self.prompts.len())
    }

    /// Accepts the current field's value as this prompt's answer.
    /// Advances to the next prompt (pre-filling its own default) and
    /// returns `None` if there is one; once every prompt has an answer,
    /// substitutes them all into `commands` and returns the finished,
    /// ready-to-run list instead.
    pub fn accept_current(&mut self) -> Option<Vec<String>> {
        self.answers.push((self.prompts[self.current].label.clone(), self.value.clone()));
        self.current += 1;

        if self.current < self.prompts.len() {
            self.value = self.prompts[self.current].default.clone();
            self.cursor = self.value.chars().count();
            self.selection_anchor = None;
            None
        } else {
            Some(self.commands.iter().map(|command| parse::substitute_prompts(command, &self.answers)).collect())
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("user-menu")
    }

    mod resolve_menu_file_tests {
        use super::*;

        #[test]
        fn none_when_neither_file_exists() {
            let dir = scratch_dir();
            assert_eq!(resolve_menu_file(&dir), None);
        }

        #[test]
        fn prefers_an_existing_litastum_menu_over_far_menu() {
            let dir = scratch_dir();
            fs::write(dir.join(OWN_FILE_NAME), "s: mine\necho mine\n").unwrap();
            fs::write(dir.join(FAR_FILE_NAME), "s: theirs\necho theirs\n").unwrap();

            let path = resolve_menu_file(&dir).unwrap();

            assert_eq!(fs::read_to_string(path).unwrap(), "s: mine\necho mine\n");
        }

        /// The actual point of the whole migration feature: a real
        /// `FarMenu.ini` should work out of the box, and litastum should
        /// keep using its own copy afterward without touching the
        /// original.
        #[test]
        fn migrates_a_far_menu_into_litastums_own_file() {
            let dir = scratch_dir();
            let far_content = "s: status\ngit status -s\n";
            fs::write(dir.join(FAR_FILE_NAME), far_content).unwrap();

            let path = resolve_menu_file(&dir).unwrap();

            assert_eq!(path, dir.join(OWN_FILE_NAME));
            assert_eq!(fs::read_to_string(&path).unwrap(), far_content);
            assert_eq!(fs::read_to_string(dir.join(FAR_FILE_NAME)).unwrap(), far_content, "the original FarMenu.ini must be left untouched");
        }
    }

    mod create_menu_file_tests {
        use super::*;

        #[test]
        fn creates_an_empty_litastum_menu_file() {
            let dir = scratch_dir();

            let path = create_menu_file(&dir).unwrap();

            assert_eq!(path, dir.join(OWN_FILE_NAME));
            assert_eq!(fs::read_to_string(&path).unwrap(), "");
        }

        #[test]
        fn the_created_file_is_then_found_by_resolve_menu_file() {
            let dir = scratch_dir();
            create_menu_file(&dir).unwrap();

            assert_eq!(resolve_menu_file(&dir), Some(dir.join(OWN_FILE_NAME)));
        }
    }

    mod user_menu_state_tests {
        use super::*;

        fn menu_with(items: Vec<MenuItem>) -> UserMenuState {
            UserMenuState { stack: vec![UserMenuLevel { items, selected: 0 }] }
        }

        fn command_item(title: &str) -> MenuItem {
            MenuItem { hotkey: None, title: title.to_string(), body: MenuItemBody::Commands(vec!["echo hi".to_string()]) }
        }

        fn submenu_item(title: &str, children: Vec<MenuItem>) -> MenuItem {
            MenuItem { hotkey: None, title: title.to_string(), body: MenuItemBody::Submenu(children) }
        }

        #[test]
        fn open_returns_none_without_a_menu_file() {
            assert!(UserMenuState::open(&scratch_dir()).is_none());
        }

        #[test]
        fn open_loads_a_real_menu_file() {
            let dir = scratch_dir();
            fs::write(dir.join(OWN_FILE_NAME), "s: status\ngit status -s\n").unwrap();

            let menu = UserMenuState::open(&dir).unwrap();

            assert_eq!(menu.current_level().items.len(), 1);
            assert_eq!(menu.current_level().items[0].title, "status");
        }

        #[test]
        fn move_down_and_up_clamp_at_the_edges() {
            let mut menu = menu_with(vec![command_item("a"), command_item("b")]);
            menu.move_down();
            assert_eq!(menu.current_level().selected, 1);
            menu.move_down();
            assert_eq!(menu.current_level().selected, 1, "clamped at the last item");
            menu.move_up();
            menu.move_up();
            assert_eq!(menu.current_level().selected, 0, "clamped at the first item");
        }

        #[test]
        fn enter_submenu_descends_and_back_restores_the_parent_level() {
            let mut menu = menu_with(vec![submenu_item("parent", vec![command_item("child")])]);

            assert!(menu.enter_submenu());
            assert_eq!(menu.current_level().items[0].title, "child");

            let stayed_in_menu = menu.back();
            assert!(stayed_in_menu);
            assert_eq!(menu.current_level().items[0].title, "parent");
        }

        #[test]
        fn enter_submenu_is_a_noop_on_a_commands_item() {
            let mut menu = menu_with(vec![command_item("leaf")]);
            assert!(!menu.enter_submenu());
        }

        #[test]
        fn back_at_the_top_level_signals_close() {
            let mut menu = menu_with(vec![command_item("a")]);
            assert!(!menu.back());
        }

        #[test]
        fn entering_a_submenu_preserves_the_parent_levels_own_cursor_position() {
            let mut menu = menu_with(vec![command_item("a"), submenu_item("b", vec![command_item("child")])]);
            menu.move_down(); // cursor on "b"

            menu.enter_submenu();
            menu.back();

            assert_eq!(menu.current_level().selected, 1, "should still be on \"b\", not reset to 0");
        }
    }

    mod user_menu_prompt_state_tests {
        use super::*;

        #[test]
        fn starts_prefilled_with_the_first_prompts_default() {
            let prompts = vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }];
            let state = UserMenuPromptState::new(vec!["git checkout !?Branch?Master!".to_string()], prompts);
            assert_eq!(state.value, "Master");
            assert_eq!(state.current_label(), "Branch");
            assert_eq!(state.progress(), (1, 1));
        }

        #[test]
        fn accept_current_advances_to_the_next_prompt() {
            let prompts = vec![
                Prompt { label: "First".to_string(), default: "a".to_string() },
                Prompt { label: "Second".to_string(), default: "b".to_string() },
            ];
            let mut state = UserMenuPromptState::new(vec!["echo !?First?a! !?Second?b!".to_string()], prompts);

            let result = state.accept_current();

            assert_eq!(result, None, "should not finish yet -- one prompt left");
            assert_eq!(state.current_label(), "Second");
            assert_eq!(state.value, "b", "pre-filled with the next prompt's own default");
        }

        #[test]
        fn accept_current_returns_the_substituted_commands_once_every_prompt_is_answered() {
            let prompts = vec![Prompt { label: "Branch".to_string(), default: "Master".to_string() }];
            let mut state = UserMenuPromptState::new(vec!["git checkout !?Branch?Master!".to_string()], prompts);
            state.value = "feature/x".to_string();

            let result = state.accept_current();

            assert_eq!(result, Some(vec!["git checkout feature/x".to_string()]));
        }
    }
}
