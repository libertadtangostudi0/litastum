//! The list that pops up above the command line while typing, as in Far:
//! matching history entries (newest first), then the active panel's
//! entries whose names start with the word being typed.

use crate::app::App;

use super::history::suggest_history;

/// Panel names listed at most -- a large directory would otherwise bury
/// the history.
const MAX_FILE_SUGGESTIONS: usize = 50;


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggestion {
    /// A whole command from the history: replaces the line.
    History(String),
    /// A name from the active panel: replaces the word being typed.
    File { name: String, is_dir: bool },
}


/// What the list shows for the current line. File names come from the
/// panel's listing, already in memory -- this runs every frame, so no
/// disk reads; a word with a path in it is left to `Tab` completion.
pub fn suggestions(app: &App) -> Vec<Suggestion> {
    let line = app.command_line.text();
    let mut list: Vec<Suggestion> = suggest_history(&app.command_history, line).into_iter().map(|entry| Suggestion::History(entry.to_string())).collect();
    let start = word_start(line);
    let word = &line[start..];
    if word.is_empty() || word.contains(['/', '\\']) {
        return list;
    }
    let word = word.to_lowercase();
    let files: Vec<Suggestion> = app.panels[app.active]
        .entries
        .iter()
        .filter(|entry| entry.name != ".." && entry.name.to_lowercase().starts_with(&word))
        .take(MAX_FILE_SUGGESTIONS)
        .map(|entry| Suggestion::File { name: entry.name.clone(), is_dir: entry.is_dir })
        .collect();
    // A history entry that is just one of these names is listed once, as
    // the name -- reported: as a history row, F4 didn't open the file.
    if start == 0 {
        list.retain(|suggestion| !files.iter().any(|file| matches!((suggestion, file), (Suggestion::History(entry), Suggestion::File { name, .. }) if entry == name)));
    }
    list.extend(files);
    list
}


/// Where the word being typed starts: after the line's last whitespace.
fn word_start(line: &str) -> usize {
    line.rfind(char::is_whitespace).map_or(0, |i| i + 1)
}


impl Suggestion {
    /// The command line after accepting this. A file ends in a space,
    /// ready for the next argument, a directory in a separator, as `Tab`
    /// completion does; a name with spaces is quoted.
    pub fn accepted(&self, line: &str) -> String {
        match self {
            Suggestion::History(entry) => entry.clone(),
            Suggestion::File { name, is_dir } => {
                let name = if name.contains(char::is_whitespace) { format!("\"{name}\"") } else { name.clone() };
                let end = if *is_dir { std::path::MAIN_SEPARATOR } else { ' ' };
                format!("{}{name}{end}", &line[..word_start(line)])
            }
        }
    }

    /// The row as the list shows it: a directory with a trailing separator.
    pub fn label(&self) -> String {
        match self {
            Suggestion::History(entry) => entry.clone(),
            Suggestion::File { name, is_dir: true } => format!("{name}{}", std::path::MAIN_SEPARATOR),
            Suggestion::File { name, .. } => name.clone(),
        }
    }
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::{test_app, unique_scratch_dir};

    const SEP: char = std::path::MAIN_SEPARATOR;

    fn app_in(files: &[&str], dirs: &[&str]) -> App {
        let dir = unique_scratch_dir("suggestions");
        for name in files {
            fs::write(dir.join(name), "").unwrap();
        }
        for name in dirs {
            fs::create_dir_all(dir.join(name)).unwrap();
        }
        test_app(dir)
    }

    /// Reported: typing the start of a file's name suggested only history.
    #[test]
    fn the_panels_names_follow_the_history() {
        let mut app = app_in(&["cmt_msg.txt", "CreateBranch.ps1"], &["cmake"]);
        app.command_history = vec!["svn commit -F cmt_msg.txt RFI14.1".into()];
        app.command_line.set_text("cm");

        assert_eq!(
            suggestions(&app),
            vec![
                Suggestion::History("svn commit -F cmt_msg.txt RFI14.1".into()),
                Suggestion::File { name: "cmake".into(), is_dir: true },
                Suggestion::File { name: "cmt_msg.txt".into(), is_dir: false },
            ]
        );
    }

    #[test]
    fn a_name_replaces_only_the_word_being_typed() {
        let mut app = app_in(&["cmt_msg.txt", "Hg clone.bat"], &["RFI14.1"]);
        app.command_line.set_text("svn commit -F cm");

        let [file] = suggestions(&app).try_into().unwrap();
        assert_eq!(file.accepted("svn commit -F cm"), "svn commit -F cmt_msg.txt ");

        let dir = Suggestion::File { name: "RFI14.1".into(), is_dir: true };
        assert_eq!(dir.accepted("cd rf"), format!("cd RFI14.1{SEP}"));
        let spaced = Suggestion::File { name: "Hg clone.bat".into(), is_dir: false };
        assert_eq!(spaced.accepted("run hg"), "run \"Hg clone.bat\" ");
    }

    #[test]
    fn no_names_for_an_empty_word_a_path_or_the_parent_entry() {
        let mut app = app_in(&["a.txt"], &[]);
        app.command_line.set_text("type ");
        assert!(suggestions(&app).is_empty());
        app.command_line.set_text("type sub\\a");
        assert!(suggestions(&app).is_empty(), "a path is Tab's job");
        app.command_line.set_text(".");
        assert!(suggestions(&app).is_empty(), "never the panel's ..");
    }

    #[test]
    fn a_name_already_in_the_history_is_listed_once_as_the_name() {
        let mut app = app_in(&["cmt_msg.txt"], &[]);
        app.command_history = vec!["svn commit -F cmt_msg.txt RFI14.1".into(), "cmt_msg.txt".into()];
        app.command_line.set_text("cmt");

        assert_eq!(
            suggestions(&app),
            vec![Suggestion::History("svn commit -F cmt_msg.txt RFI14.1".into()), Suggestion::File { name: "cmt_msg.txt".into(), is_dir: false }]
        );
    }
}
