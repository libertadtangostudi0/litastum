use std::io::Stdout;

use color_eyre::eyre::Result;
use ratatui::{prelude::CrosstermBackend, Terminal};

use crate::app::App;

use super::browsing::{run_shell_command_lines, toggle_panels_hidden};


/// Work a key handler needs done on the real terminal. Handlers return
/// it instead of taking the `Terminal` themselves, so they stay
/// testable without a console; `event_loop` performs it
/// (`apply_effect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Run these lines through the active shell
    /// profile (`run_shell_command_lines`, which also handles `cd` lines).
    RunShell(Vec<String>),
    /// `cls`/`clear`: repaint from scratch.
    ClearScreen,
    /// `Ctrl+O`: show the user screen until `Ctrl+O` again.
    ToggleHiddenPanels,
}


pub(crate) fn apply_effect(app: &mut App, terminal: &mut Terminal<CrosstermBackend<Stdout>>, effect: Effect) -> Result<()> {
    match effect {
        Effect::None => Ok(()),
        Effect::RunShell(lines) => run_shell_command_lines(app, terminal, &lines),
        Effect::ClearScreen => {
            app.user_screen.clear();
            terminal.clear()?;
            app.active_panel().reload()?;
            Ok(())
        }
        Effect::ToggleHiddenPanels => toggle_panels_hidden(app, terminal),
    }
}
