use edtui::{EditorMode, EditorState, Lines};

use super::extend_word_selection;

fn state_for(contents: &str, cursor_col: usize) -> EditorState {
    let mut state = EditorState::new(Lines::from(contents));
    state.mode = EditorMode::Insert;
    state.cursor.col = cursor_col;
    state
}

/// Real reported text (`"- App: owns panels, active index"`,
/// architecture-doc-shaped) -- unlike every test above, this mixes
/// in punctuation (`:`, `,`) between words, which `edtui`'s own
/// 3-way character classification (word / punctuation / whitespace)
/// treats as its own single-character "word". Repeated
/// `Ctrl+Shift+Left` from the end of the line should still strictly
/// monotonically extend the selection leftward, landing on each
/// word *and* each punctuation run in turn, never stalling or
/// jumping backward.
#[test]
fn repeated_left_monotonically_extends_through_punctuation() {
    let text = "- App: owns panels, active index";
    let mut state = state_for(text, text.chars().count());

    let mut previous = state.cursor.col;
    for _ in 0..8 {
        extend_word_selection(&mut state, false, false);
        let sel = state.selection.as_ref().expect("should have a selection");
        assert!(sel.end.col < previous, "should keep moving left, not stall or reverse: {previous} -> {}", sel.end.col);
        assert_eq!(sel.end, state.cursor, "left of the anchor, the selection end should exactly track the cursor");
        previous = sel.end.col;
        if previous == 0 {
            break;
        }
    }
}

/// The colon right after "App" is its own single-character
/// "word" (punctuation class, distinct from the surrounding word
/// characters) -- confirms it's a real, individually-selectable
/// stop, not silently merged into "App" or into the following
/// whitespace.
#[test]
fn colon_is_its_own_word_stop() {
    let text = "App: owns";
    let mut state = state_for(text, text.chars().count());

    extend_word_selection(&mut state, false, false); // "owns"
    extend_word_selection(&mut state, false, false); // ":"
    let sel = state.selection.as_ref().expect("should have a selection");
    assert_eq!(sel.end.col, 3, "should land on the ':' itself, index 3");
    assert_eq!(&text[3..4], ":");
}

/// Forward selection through punctuation must stop cleanly too --
/// `"App:"` has no space between `"App"` and `':'`, so the first
/// press lands right on the second `'p'`, not swallowing the `':'`
/// into the same selection (`MoveWordForwardToEndOfWord` breaks on
/// the class change, same as it does for whitespace). The very next
/// press must still make progress onto the `':'` itself rather than
/// stalling right at that boundary.
#[test]
fn repeated_right_extends_through_punctuation_with_no_extra_character() {
    let text = "App: owns panels";
    let mut state = state_for(text, 0);

    extend_word_selection(&mut state, true, false); // "App"
    let after_first = state.selection.as_ref().expect("should have a selection").end.col;
    assert_eq!(after_first, 2, "should land on the second 'p' of \"App\", not swallow the ':'");

    extend_word_selection(&mut state, true, false); // ":"
    let after_second = state.selection.as_ref().expect("should still have a selection").end.col;
    assert!(after_second > after_first, "second press should make progress onto the ':' itself: {after_first} -> {after_second}");
}
