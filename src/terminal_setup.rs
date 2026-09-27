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
    // A thin bar matches a normal text caret; edtui's own cursor
    // highlight is turned off in editor.rs so this is what's visible
    // while editing (blinking, so it's still findable at a glance).
    //
    // `EnableBracketedPaste` -- reported directly as a real, severe
    // performance problem: pasting a multi-hundred-line file into the
    // built-in editor "felt like watching it render line by line" and
    // took well over a minute, even after `editor::fast_paste_from_clipboard`
    // made the actual splice itself sub-millisecond
    // (`editor/editor/fast_paste.rs`). Root cause was one level further
    // out than the buffer-insert algorithm: without bracketed-paste
    // mode, the terminal has no way to tell this app "this whole block
    // arrived from a paste, not a human typing" -- it just feeds every
    // character of the pasted text through as its own separate
    // `Event::Key` press. Each one is a real keystroke as far as this
    // app is concerned, hitting the *normal* per-character `InsertChar`
    // path (never `fast_paste_from_clipboard`'s own `Ctrl+V` interception
    // at all, since there's no `Ctrl+V` keypress anywhere in this
    // stream to intercept) and triggering `event_loop::run`'s own full
    // per-event redraw every single time -- thousands of characters,
    // thousands of redraws, is exactly what "line by line" rendering
    // looks like from the outside. `EnableBracketedPaste` asks the
    // terminal to instead wrap a paste in `ESC[200~.../ESC[201~` and
    // hand it to `crossterm` as one single `Event::Paste(String)` --
    // handled in `handle_event`, below.
    execute!(stdout, EnterAlternateScreen, SetCursorStyle::BlinkingBar, EnableBracketedPaste)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}


pub(crate) fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>, mouse_capture_enabled: bool) -> Result<()> {
    disable_raw_mode()?;
    // `DisableMouseCapture` here is a safety net, not the primary
    // toggle -- mouse capture is normally turned on/off around just the
    // Markdown preview session itself
    // (`explorer::markdown_preview::open_preview`/
    // `handle_markdown_preview_key`), so it doesn't interfere with
    // native mouse text-selection everywhere else in this app. But
    // quitting (`F10`) while a preview happens to still be open would
    // otherwise skip that "turn it back off" step and leak mouse
    // capture into the user's terminal after this process exits.
    //
    // `mouse_capture_enabled` (`app.mouse_capture_enabled`) gates
    // whether `DisableMouseCapture` is even attempted -- reported as a
    // real crash on Windows (`Error: 0: Initial console modes not set`)
    // from sending it *unconditionally*: `crossterm`'s Windows console
    // backend has no "initial mode" saved to restore unless
    // `EnableMouseCapture` actually ran first in this process, and
    // quitting from a plain `Mode::Browsing` session (never having
    // opened a Markdown preview at all) hit exactly that case.
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
