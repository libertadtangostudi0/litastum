//! Clamped-cursor movement for a plain, flat list-picker popup.
//!
//! The same two lines (`selected = selected.saturating_sub(1)` on the
//! way up, `if selected + 1 < len { selected += 1 }` on the way down)
//! were hand-copied across roughly eight list-popup states in this
//! codebase (`theming::{ThemeMenu, MainMenu, PopupStyleMenu}`,
//! `command_line::{ShellMenu, CommandHistoryMenu}`,
//! `explorer::DriveMenu`, `editor::{EditorKeymapMenu, EditorMenu}`),
//! in two different shapes: a `move_up`/`move_down` method on the
//! menu's own struct, or inlined directly in the key handler. Neither
//! shape needed anything beyond `&mut usize` and a length, so there
//! was nothing to gain from wrapping `selected` in a dedicated type --
//! these two free functions are the whole thing.

/// Moves `selected` up by one row, clamped at `0` -- never panics on
/// an already-empty list (`saturating_sub`).
pub fn move_up(selected: &mut usize) {
    *selected = selected.saturating_sub(1);
}

/// Moves `selected` down by one row, clamped at `len - 1` (a no-op on
/// an empty list, since `0 + 1 < 0` is never true).
pub fn move_down(selected: &mut usize, len: usize) {
    if *selected + 1 < len {
        *selected += 1;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_up_clamps_at_zero() {
        let mut selected = 0;
        move_up(&mut selected);
        assert_eq!(selected, 0);

        let mut selected = 3;
        move_up(&mut selected);
        assert_eq!(selected, 2);
    }

    #[test]
    fn move_down_clamps_at_len_minus_one() {
        let mut selected = 2;
        move_down(&mut selected, 3);
        assert_eq!(selected, 2, "already at the last index (len 3, indices 0..=2)");

        let mut selected = 0;
        move_down(&mut selected, 3);
        assert_eq!(selected, 1);
    }

    #[test]
    fn move_down_on_an_empty_list_is_a_noop() {
        let mut selected = 0;
        move_down(&mut selected, 0);
        assert_eq!(selected, 0);
    }
}
