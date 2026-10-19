//! Commands that delete a lot at once, without asking -- `rd /s`,
//! `del /s`, `format`, `rm -rf`, `Remove-Item -Recurse` -- asked about
//! before they run (requested; `Settings::confirm_dangerous_commands`
//! turns it off). By their words, each part of a chain (`&&`, `&`, `|`,
//! `;`) on its own.


use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;

use crate::app::{App, Overlay, PendingCommand};
use crate::yes_no::{self, Answer};

use super::Effect;


/// `line` asks first if it's dangerous and the setting is on: the prompt
/// opens, and `true`.
pub(super) fn ask_first(app: &mut App, line: &str) -> bool {
    let Some(reason) = danger(line).filter(|_| app.settings.confirm_dangerous_commands) else {
        return false;
    };
    app.overlay = Some(Overlay::ConfirmCommand(PendingCommand { line: line.to_string(), reason }));
    true
}


/// The prompt's keys: `Y` runs the command, `N`/`Esc` don't.
pub fn handle_confirm_command_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    match yes_no::answer(key) {
        Answer::Yes => {
            let Some(Overlay::ConfirmCommand(pending)) = app.overlay.take() else {
                return Ok(Effect::None);
            };
            Ok(Effect::RunShell(vec![pending.line]))
        }
        Answer::No => {
            app.overlay = None;
            Ok(Effect::None)
        }
        Answer::Ignore => Ok(Effect::None),
    }
}


/// Why `line` asks first -- what it does -- or `None` if it doesn't.
pub fn danger(line: &str) -> Option<&'static str> {
    line.split(['&', '|', ';']).find_map(part_danger)
}


fn part_danger(part: &str) -> Option<&'static str> {
    let words: Vec<String> = part.split_whitespace().map(str::to_lowercase).collect();
    let (command, args) = words.split_first()?;
    let command = command.trim_start_matches('@');
    let has = |flag: &str| args.iter().any(|arg| arg == flag || arg.starts_with(&format!("{flag}:")));
    // Short Unix flags run together: `-rf`, `-fr`, `-Rf`.
    let short_flags = |letter: char| args.iter().any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains(letter));
    match command {
        "rd" | "rmdir" if has("/s") => Some("deletes a directory with all in it"),
        "del" | "erase" if has("/s") => Some("deletes files in every subdirectory"),
        "del" | "erase" if has("/q") && args.iter().any(|arg| arg.contains('*')) => Some("deletes every matching file without asking"),
        "format" if args.iter().any(|arg| arg.len() == 2 && arg.ends_with(':')) => Some("formats a drive"),
        "rm" if short_flags('r') || has("--recursive") || has("-recurse") => Some("deletes a directory with all in it"),
        "remove-item" | "ri" | "rmdir" | "del" if has("-recurse") || has("-r") => Some("deletes a directory with all in it"),
        "diskpart" => Some("changes disks and partitions"),
        "cipher" if has("/w") => Some("wipes a drive's free space"),
        _ => None,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_that_delete_a_lot_are_told() {
        for line in [
            "rd /s /q build",
            "RMDIR /S target",
            "del /s *.obj",
            "del /q *.*",
            "format D:",
            "rm -rf node_modules",
            "rm -fr dist",
            "rm --recursive out",
            "Remove-Item -Recurse -Force .\\bin",
            "diskpart",
            "cipher /w:C:",
            "cd build && rd /s /q obj",
        ] {
            assert!(danger(line).is_some(), "{line}");
        }
    }

    #[test]
    fn everyday_commands_are_not() {
        for line in ["rd empty", "del a.txt", "del /q a.txt", "rm a.txt", "dir /s", "hg pull", "git status", "cargo clean", "format", "echo rd /s"] {
            assert_eq!(danger(line), None, "{line}");
        }
    }

    /// Requested: a command that deletes a lot asks; `Y` runs it, `Esc`
    /// doesn't; with the setting off it runs at once.
    #[test]
    fn a_dangerous_command_asks_and_runs_on_y() {
        use crate::test_support::{key, test_app, unique_scratch_dir};
        use crossterm::event::KeyCode;

        let mut app = test_app(unique_scratch_dir("dangerous"));
        assert!(ask_first(&mut app, "rd /s /q build"));
        assert!(matches!(app.overlay, Some(Overlay::ConfirmCommand(_))));
        assert_eq!(handle_confirm_command_key(&mut app, key(KeyCode::Char('y'))).unwrap(), Effect::RunShell(vec!["rd /s /q build".to_string()]));
        assert!(app.overlay.is_none());

        assert!(ask_first(&mut app, "rd /s /q build"));
        assert_eq!(handle_confirm_command_key(&mut app, key(KeyCode::Esc)).unwrap(), Effect::None);
        assert!(app.overlay.is_none());

        app.settings.confirm_dangerous_commands = false;
        assert!(!ask_first(&mut app, "rd /s /q build"));
        assert!(!ask_first(&mut app, "dir"));
    }
}
