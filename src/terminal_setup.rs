use std::io::{self, Stdout};

use color_eyre::eyre::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};
use tracing::debug;

/// Reported directly: `Ctrl+C` pressed while a shelled-out command was
/// running (`command_line::run_shell_command_lines` /
/// `toggle_panels_hidden`'s own console loop -- both leave raw mode and
/// hand the console to a real child process) took the whole litastum
/// process down with it, not just the child. Root cause: raw mode is
/// what normally keeps `Ctrl+C` from ever becoming an OS-level signal
/// at all -- `enable_raw_mode` clears `ENABLE_PROCESSED_INPUT` on
/// Windows / `ISIG` on Unix, so while the TUI itself has raw mode on,
/// `Ctrl+C` arrives as an ordinary `KeyEvent` like any other key, never
/// a signal. The moment raw mode is turned back off to run a real
/// subprocess with inherited stdio, that protection goes away too --
/// `Ctrl+C` becomes a real `CTRL_C_EVENT`/`SIGINT` again, delivered to
/// *every* process still attached to the same console/process group,
/// our own included. With no handler of our own installed, the
/// platform's default action for that signal is to terminate the
/// process -- so litastum died right along with the child it was
/// waiting on.
///
/// Installing an otherwise-empty handler here doesn't suppress the
/// signal for the *child* -- it's delivered to that process
/// independently, and an ordinary CLI tool's own default disposition
/// (interrupt/exit) still applies to it exactly as if it had been run
/// directly in a real shell. It only stops *our* process from dying
/// alongside it: the child gets interrupted, `Command::status()`
/// returns once it exits, and control comes back to whichever of our
/// own wrapper functions was waiting on it -- the same "Ctrl+C
/// interrupts the foreground command, not the shell itself" behavior
/// every real shell already has. `ctrlc` (rather than hand-rolling
/// `SetConsoleCtrlHandler`/`sigaction` behind `#[cfg(windows)]`/
/// `#[cfg(unix)]` ourselves) covers both platforms' actual mechanism
/// with one call.
pub(crate) fn install_ctrl_c_handler() -> Result<()> {
    ctrlc::set_handler(|| debug!("Ctrl+C received; ignored at the litastum process level"))?;
    Ok(())
}


pub(crate) fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
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
    execute!(stdout, EnterAlternateScreen, SetCursorStyle::BlinkingBar, EnableBracketedPaste)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}


pub(crate) fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mouse_capture_enabled: bool) -> Result<()> {
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
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape
    )?;
    Ok(())
}
