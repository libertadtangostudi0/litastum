use std::io::{self, BufWriter, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

/// With raw mode off for a child process, `Ctrl+C` is a real OS signal
/// for every process on the console, and with no handler it killed
/// litastum too. An empty handler keeps us alive; the child is still
/// interrupted as in a real shell. History: docs/history/command-execution.md.
pub(crate) fn install_ctrl_c_handler() -> Result<()> {
    ctrlc::set_handler(|| debug!("Ctrl+C received; ignored at the litastum process level"))?;
    Ok(())
}


/// The terminal litastum draws on. Its output is buffered whole and
/// written once per frame (`ratatui` flushes after each draw): `Stdout`
/// flushes every 1 KiB, so a frame reached the terminal in pieces and a
/// window could show it half drawn -- one Compare pane scrolled, the
/// other not yet (requested).
pub(crate) type Tui = Terminal<CrosstermBackend<BufWriter<Stdout>>>;

/// Big enough for a full frame of a large window.
const FRAME_BUFFER_BYTES: usize = 1 << 20;


pub(crate) fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // A thin bar matches a normal text caret (blinking, so it's still
    // findable at a glance).
    //
    // `EnableBracketedPaste` delivers a paste as one `Event::Paste`
    // (`event_loop::paste::handle_paste_event`) instead of one key event
    // and redraw per character. Unix only in practice: `crossterm`'s
    // Windows backend never produces `Event::Paste` -- Windows goes
    // through `windows_terminal::paste_hotkey` instead. History:
    // docs/history/editor-performance.md.
    //
    // `SetTitle`: the window or tab reads "litastum" rather than the path
    // of the exe (docs/history/launching.md).
    //
    // `EnableFocusChange`: the terminal says when it gains and loses the
    // focus (`App::terminal_focused`), so the polled `Ctrl+V` ignores a
    // press in another program.
    execute!(stdout, EnterAlternateScreen, SetCursorStyle::BlinkingBar, EnableBracketedPaste, EnableFocusChange, SetTitle("litastum"))?;
    Ok(Terminal::new(CrosstermBackend::new(BufWriter::with_capacity(FRAME_BUFFER_BYTES, stdout)))?)
}


pub(crate) fn restore_terminal(terminal: &mut Tui, mouse_capture_enabled: bool) -> Result<()> {
    disable_raw_mode()?;
    // `DisableMouseCapture` is a safety net: capture is normally synced
    // to the current mode (`event_loop::sync_mouse_capture`), but
    // quitting mid-session would otherwise leak it into the user's
    // terminal. Sent only if `EnableMouseCapture` actually succeeded
    // earlier -- on Windows, disabling without it crashes with
    // "Initial console modes not set".
    if mouse_capture_enabled {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape,
        crossterm::style::Print(crate::terminal_palette::RESET_PALETTE)
    )?;
    Ok(())
}
