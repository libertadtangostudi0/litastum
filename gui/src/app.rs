use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::term::TermMode;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{Ime, MouseButton, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::font::CellFont;
use crate::input::{self, WheelAccumulator, Zoom};
use crate::render::{self, Renderer, View};
use crate::session::{GridSize, Session, UserEvent};
use crate::tabs::Tabs;
use crate::window_style;
use crate::zoom::Zooms;

mod keyboard;
mod pointer;
mod tab_bar;

/// The starting font size in logical pixels (`Ctrl+0`); scaled by the
/// monitor's DPI.
const FONT_SIZE: f32 = 15.0;
/// Half a blink: how long the cursor stays shown, then hidden.
const BLINK_INTERVAL: Duration = Duration::from_millis(530);
/// The window's first size, in cells.
const START_GRID: GridSize = GridSize { columns: 140, lines: 40 };


/// The window and its tabs. The window, surface, font and tabs exist once
/// the event loop has resumed (winit creates windows only then) --
/// `Shown`.
pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    shown: Option<Shown>,
    /// Why there's no session; its window, titled with it.
    failure: Option<(String, Option<Window>)>,
    modifiers: ModifiersState,
    /// The cell under the mouse pointer, and the button held down for
    /// drag reports.
    pointer_cell: (usize, usize),
    /// The column of the cell edge nearest the pointer, for a click in
    /// litastum's editors (`mouse::clicks_snap_to_edges`).
    pointer_edge_column: usize,
    /// The tab bar's column under the pointer, when it's over the bar.
    pointer_on_bar: Option<usize>,
    held_button: Option<MouseButton>,
    wheel: WheelAccumulator,
    /// Whether a blinking cursor is in its shown half, and when it flips.
    cursor_shown: bool,
    next_blink: Instant,
    /// The background and text colors the window frame was last given.
    frame_colors: Option<(alacritty_terminal::vte::ansi::Rgb, alacritty_terminal::vte::ansi::Rgb)>,
    /// The font size in logical pixels now: the shown screen's zoom.
    font_size: f32,
    /// Each litastum screen's zoom, kept for the user (`zoom`).
    zooms: Zooms,
    /// The last tab closed: the event loop ends when it next can.
    exit_requested: bool,
}

struct Shown {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    font: CellFont,
    tabs: Tabs<Session>,
    renderer: Renderer,
    grid: GridSize,
    /// The console litastum every tab runs, and where it starts.
    program: PathBuf,
    working_directory: PathBuf,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            shown: None,
            failure: None,
            modifiers: ModifiersState::empty(),
            pointer_cell: (0, 0),
            pointer_edge_column: 0,
            pointer_on_bar: None,
            held_button: None,
            wheel: WheelAccumulator::default(),
            cursor_shown: true,
            next_blink: Instant::now() + BLINK_INTERVAL,
            frame_colors: None,
            font_size: Zooms::load(FONT_SIZE).size_for(crate::zoom::MAIN_SCREEN),
            zooms: Zooms::load(FONT_SIZE),
            exit_requested: false,
        }
    }

    fn open(&self, event_loop: &ActiveEventLoop) -> Result<Shown, String> {
        let mut font = CellFont::new(self.font_size);
        let start = LogicalSize::new(START_GRID.columns as u32 * font.cell_width, (START_GRID.lines as u32 + 1) * font.cell_height);
        let attributes = window_style::themed(Window::default_attributes().with_title("litastum").with_inner_size(start));
        let window = Rc::new(event_loop.create_window(attributes).map_err(|err| err.to_string())?);
        // Committed IME text (Win+. emoji, CJK input) arrives as `Ime`.
        window.set_ime_allowed(true);

        font.set_size(self.font_size * window.scale_factor() as f32);
        let context = softbuffer::Context::new(window.clone()).map_err(|err| err.to_string())?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|err| err.to_string())?;
        let grid = grid_for(window.inner_size(), &font);

        let program = crate::console_litastum().ok_or("litastum's console program wasn't found next to this one")?;
        let working_directory = std::env::current_dir().unwrap_or_default();
        let mut shown = Shown { window, surface, font, tabs: Tabs::new(), renderer: Renderer::new(), grid, program, working_directory };
        tab_bar::spawn_tab(&mut shown, &self.proxy).map_err(|err| format!("couldn't start {}: {err}", shown.program.display()))?;
        Ok(shown)
    }

    fn session(&self) -> Option<&Session> {
        Some(&self.shown.as_ref()?.tabs.active()?.session)
    }

    fn mode(&self) -> TermMode {
        self.session().map_or(TermMode::empty(), |session| *session.term.lock().mode())
    }

    fn write(&self, bytes: impl Into<Vec<u8>>) {
        if let Some(session) = self.session() {
            session.write(bytes.into());
        }
    }

    fn request_redraw(&self) {
        if let Some(shown) = &self.shown {
            shown.window.request_redraw();
        }
    }

    /// A new window size; `cell_changed` after a DPI change, when the
    /// grid can stay the same while every cell's pixel size doesn't.
    /// Every tab gets it, so a tab switched to later is already right.
    fn resized(&mut self, size: PhysicalSize<u32>, cell_changed: bool) {
        let Some(shown) = &mut self.shown else {
            return;
        };
        let grid = grid_for(size, &shown.font);
        if grid != shown.grid || cell_changed {
            shown.grid = grid;
            let cell = (shown.font.cell_width, shown.font.cell_height);
            for tab in shown.tabs.iter_mut() {
                tab.session.resize(grid, cell);
            }
        }
        shown.window.request_redraw();
    }

    fn redraw(&mut self) {
        let cursor_shown = self.cursor_shown;
        let Some(shown) = &mut self.shown else {
            return;
        };
        let size = shown.window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if shown.surface.resize(width, height).is_err() {
            return;
        }
        let Ok(mut buffer) = shown.surface.buffer_mut() else {
            return;
        };
        let Some(tab) = shown.tabs.active() else {
            return;
        };
        let bar = {
            let mut term = tab.session.term.lock();
            let cursor_visible = cursor_shown || !term.cursor_style().blinking;
            let origin = render::grid_origin(size.width, shown.grid.columns, shown.font.cell_width, shown.font.cell_height);
            let view = View { width: size.width, height: size.height, origin, cursor_visible };
            let mut images = tab.session.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            shown.renderer.draw(&mut term, &mut shown.font, &mut images, view);
            tab_bar::cells(&shown.tabs, shown.grid.columns, term.colors())
        };
        shown.renderer.draw_bar(&mut shown.font, &bar.cells, bar.background);
        let pixels = shown.renderer.pixels();
        if buffer.len() == pixels.len() {
            buffer.copy_from_slice(pixels);
        }
        let _ = buffer.present();
    }

    fn cursor_blinks(&self) -> bool {
        self.session().is_some_and(|session| session.term.lock().cursor_style().blinking)
    }

    /// `F11`: borderless full screen on the window's monitor, and back.
    /// The resize that follows gives litastum the new grid.
    fn toggle_fullscreen(&self) {
        let Some(shown) = &self.shown else {
            return;
        };
        let fullscreen = match shown.window.fullscreen() {
            Some(_) => None,
            None => Some(Fullscreen::Borderless(shown.window.current_monitor())),
        };
        shown.window.set_fullscreen(fullscreen);
    }

    /// `Ctrl+=`/`Ctrl+-`/`Ctrl+0` or `Ctrl`+wheel: a bigger or smaller
    /// font in the same window, so more or fewer cells -- litastum gets
    /// the new grid size as for any resize. Kept for the shown tab's
    /// screen only (requested: Compare and the resolver zoom on their
    /// own), and saved.
    fn zoom(&mut self, zoom: Zoom) {
        self.font_size = input::zoomed(self.font_size, zoom, FONT_SIZE);
        let screen = self.active_screen();
        self.zooms.set(&screen, self.font_size);
        if let Some(scale_factor) = self.shown.as_ref().map(|shown| shown.window.scale_factor()) {
            self.scale_changed(scale_factor);
        }
    }

    /// The shown screen's zoom, when it isn't the one in use: after a
    /// switch to another tab or another litastum screen.
    fn sync_zoom(&mut self) {
        let wanted = self.zooms.size_for(&self.active_screen());
        if (wanted - self.font_size).abs() < f32::EPSILON {
            return;
        }
        self.font_size = wanted;
        if let Some(scale_factor) = self.shown.as_ref().map(|shown| shown.window.scale_factor()) {
            self.scale_changed(scale_factor);
        }
    }

    /// The font at `font_size` for this monitor's scale: after a zoom, or
    /// moving to a monitor with another DPI.
    fn scale_changed(&mut self, scale_factor: f64) {
        let Some(shown) = &mut self.shown else {
            return;
        };
        shown.font.set_size(self.font_size * scale_factor as f32);
        shown.renderer.invalidate();
        let size = shown.window.inner_size();
        self.resized(size, true);
    }

    fn text_area_size(&self) -> Option<WindowSize> {
        let shown = self.shown.as_ref()?;
        Some(WindowSize {
            num_lines: shown.grid.lines as u16,
            num_cols: shown.grid.columns as u16,
            cell_width: shown.font.cell_width as u16,
            cell_height: shown.font.cell_height as u16,
        })
    }

}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.shown.is_some() || self.failure.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(shown) => {
                self.shown = Some(shown);
                self.after_tab_change();
            }
            Err(failure) => {
                // No console to print to: say it in a window title.
                let attributes = window_style::themed(Window::default_attributes().with_title(format!("litastum: {failure}")));
                self.failure = Some((failure, event_loop.create_window(attributes).ok()));
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resized(size, false),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.scale_changed(scale_factor),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(event),
            WindowEvent::Ime(Ime::Commit(text)) => self.write(text),
            // Only the shown tab has the focus: a background litastum
            // must not take a Ctrl+V (it polls the key system-wide).
            WindowEvent::Focused(focused) => {
                if let Some(report) = input::focus_report(focused, self.mode()) {
                    self.write(report);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.pointer_moved(position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => self.mouse_button(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
        if self.exit_requested {
            event_loop.exit();
        }
    }

    /// The blink timer firing: flip the cursor and wait for the next one.
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            self.cursor_shown = !self.cursor_shown;
            self.next_blink = Instant::now() + BLINK_INTERVAL;
            self.request_redraw();
        }
    }

    /// Sleeps until the next blink while the cursor blinks, else until
    /// an event.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exit_requested {
            event_loop.exit();
            return;
        }
        if self.cursor_blinks() {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_blink));
        } else {
            self.cursor_shown = true;
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Term(id, event) => self.term_event(id, event),
            UserEvent::UserVar(id, name, value) => self.set_user_var(id, &name, value),
        }
        if self.exit_requested {
            event_loop.exit();
        }
    }
}


/// The window's size in whole cells under the tab bar (a cell's height),
/// at least one of each.
fn grid_for(size: PhysicalSize<u32>, font: &CellFont) -> GridSize {
    let height = size.height.saturating_sub(font.cell_height);
    GridSize { columns: (size.width / font.cell_width).max(1) as usize, lines: (height / font.cell_height).max(1) as usize }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_whole_cells_under_the_tab_bar_and_never_empty() {
        let font = CellFont::new(16.0);
        let size = PhysicalSize::new(font.cell_width * 10 + font.cell_width / 2, font.cell_height * 4);
        assert_eq!(grid_for(size, &font), GridSize { columns: 10, lines: 3 }, "a row for the tab bar");
        assert_eq!(grid_for(PhysicalSize::new(1, 1), &font), GridSize { columns: 1, lines: 1 });
    }
}
