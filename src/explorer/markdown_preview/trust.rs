//! Asking before a Markdown link is opened (requested): a link may be any
//! URL scheme a program registered, a UNC path or a script next to the
//! document. A link not trusted yet asks (`Overlay::ConfirmOpenLink`),
//! showing where it really goes -- not just its text, which can say
//! anything. `T` trusts it for good: it's kept in `appdata/trusted_links.txt`,
//! newest last, at most `MAX_TRUSTED` (the oldest drop off).

use std::path::Path;

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tracing::{debug, warn};

use crate::app::{App, Overlay};
use crate::command_line::Effect;

use super::links::{label_for_url, resolve_link_target, LinkTarget};
use super::state::MarkdownPreviewState;

/// Trusted links kept; trusting one more drops the oldest.
const MAX_TRUSTED: usize = 200;

/// The file name inside `app_data_dir()`.
#[cfg(not(test))]
const TRUSTED_FILE: &str = "trusted_links.txt";


/// A link waiting for an answer: its text, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLink {
    label: String,
    target: LinkTarget,
}

impl PendingLink {
    #[cfg(test)]
    pub fn url(label: &str, url: &str) -> Self {
        Self { label: label.to_string(), target: LinkTarget::Url(url.to_string()) }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Where the link really goes: the URL, or the file's full path.
    pub fn destination(&self) -> String {
        self.target.key()
    }
}


impl LinkTarget {
    /// What a trusted link is remembered by: the target, never the text.
    fn key(&self) -> String {
        match self {
            LinkTarget::Url(url) => url.clone(),
            LinkTarget::File(path) => path.display().to_string(),
        }
    }
}


/// Opens `url` from the preview (a click or the link search) if it's
/// trusted, else asks. The outcome goes to the preview's link message.
pub(super) fn request_open_link(app: &mut App, url: &str) {
    let Some(state) = &mut app.markdown_edit_preview else {
        return;
    };
    let markdown_dir = state.path().parent().map(Path::to_path_buf).unwrap_or_default();
    let label = label_for_url(state, url);
    let Some(target) = resolve_link_target(url, &markdown_dir) else {
        debug!(url, "markdown preview: link has no openable target (anchor, or relative file not found)");
        state.set_link_message(format!("Can't open yet: {label} (in-document anchor, or file not found)"));
        return;
    };
    if is_trusted(&target.key()) {
        open(state, &label, &target);
    } else {
        debug!(url, "markdown preview: link not trusted yet, asking");
        app.overlay = Some(Overlay::ConfirmOpenLink(PendingLink { label, target }));
    }
}


/// `Y` opens the link this once, `T` trusts it and opens it, `N`/`Esc`
/// open nothing.
pub fn handle_confirm_open_link_key(app: &mut App, key: KeyEvent) -> Result<Effect> {
    let Some(Overlay::ConfirmOpenLink(pending)) = &app.overlay else {
        return Ok(Effect::None);
    };
    let trust = match key.code {
        KeyCode::Char('y' | 'Y') => false,
        KeyCode::Char('t' | 'T') => true,
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.overlay = None;
            return Ok(Effect::None);
        }
        _ => return Ok(Effect::None),
    };
    let pending = pending.clone();
    app.overlay = None;
    if trust {
        remember(pending.target.key());
    }
    if let Some(state) = &mut app.markdown_edit_preview {
        open(state, &pending.label, &pending.target);
    }
    Ok(Effect::None)
}


fn open(state: &mut MarkdownPreviewState, label: &str, target: &LinkTarget) {
    debug!(target = %target.key(), "markdown preview: opening link");
    match launch(target) {
        Ok(()) => state.set_link_message(format!("Opened: {label}")),
        Err(err) => {
            warn!(target = %target.key(), %err, "failed to open a markdown preview link");
            state.set_link_message(format!("Failed to open {label}: {err}"));
        }
    }
}


#[cfg(not(test))]
fn launch(target: &LinkTarget) -> std::io::Result<()> {
    match target {
        LinkTarget::Url(url) => crate::explorer::system_open::open_url(url).map(drop),
        LinkTarget::File(path) => crate::explorer::system_open::open(path).map(drop),
    }
}

/// Tests never start a real browser: what would be opened is recorded.
#[cfg(test)]
fn launch(target: &LinkTarget) -> std::io::Result<()> {
    test_store::LAUNCHED.with(|launched| launched.borrow_mut().push(target.key()));
    Ok(())
}


fn is_trusted(key: &str) -> bool {
    load_trusted().iter().any(|trusted| trusted == key)
}


/// Trusts `key`: moved to the end if it's there, the oldest dropped past
/// `MAX_TRUSTED`.
fn remember(key: String) {
    let mut trusted = load_trusted();
    add_trusted(&mut trusted, key);
    save_trusted(&trusted);
}


fn add_trusted(trusted: &mut Vec<String>, key: String) {
    trusted.retain(|existing| *existing != key);
    trusted.push(key);
    if trusted.len() > MAX_TRUSTED {
        let excess = trusted.len() - MAX_TRUSTED;
        trusted.drain(..excess);
    }
}


/// The trusted links, oldest first; none if the file is missing or
/// unreadable.
#[cfg(not(test))]
pub(crate) fn load_trusted() -> Vec<String> {
    let Some(dir) = crate::app_data::app_data_dir() else {
        return Vec::new();
    };
    match std::fs::read_to_string(dir.join(TRUSTED_FILE)) {
        Ok(contents) => contents.lines().filter(|line| !line.is_empty()).map(str::to_string).collect(),
        Err(err) => {
            debug!(%err, "trusted links: none saved yet");
            Vec::new()
        }
    }
}

/// Best-effort: a failed write is logged; the link still opens.
#[cfg(not(test))]
pub(crate) fn save_trusted(trusted: &[String]) {
    let Some(dir) = crate::app_data::app_data_dir() else {
        return;
    };
    let mut contents = trusted.join("\n");
    contents.push('\n');
    if let Err(err) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(TRUSTED_FILE), contents)) {
        warn!(%err, "trusted links: failed to save");
    }
}

/// In tests the list lives in memory, per test thread.
#[cfg(test)]
pub(crate) fn load_trusted() -> Vec<String> {
    test_store::TRUSTED.with(|trusted| trusted.borrow().clone())
}

#[cfg(test)]
pub(crate) fn save_trusted(trusted: &[String]) {
    test_store::TRUSTED.with(|stored| *stored.borrow_mut() = trusted.to_vec());
}


#[cfg(test)]
pub(super) mod test_store {
    use std::cell::RefCell;

    thread_local! {
        pub(in super::super) static TRUSTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        pub(in super::super) static LAUNCHED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// What `launch` was asked to open so far, on this thread.
    pub(in super::super) fn launched() -> Vec<String> {
        LAUNCHED.with(|launched| launched.borrow().clone())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusting_again_moves_a_link_to_the_end() {
        let mut trusted = vec!["a".to_string(), "b".to_string()];
        add_trusted(&mut trusted, "a".to_string());
        assert_eq!(trusted, vec!["b", "a"]);
    }

    #[test]
    fn past_the_cap_the_oldest_links_drop_off() {
        let mut trusted: Vec<String> = (0..MAX_TRUSTED).map(|i| i.to_string()).collect();
        add_trusted(&mut trusted, "new".to_string());
        assert_eq!(trusted.len(), MAX_TRUSTED);
        assert_eq!(trusted.first().unwrap(), "1", "the oldest went");
        assert_eq!(trusted.last().unwrap(), "new");
    }
}
