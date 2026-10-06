use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::colors;
use crate::font::CellFont;
use crate::input::{self, WheelAccumulator, Zoom};
use crate::mouse::{self, MouseAction};
use crate::render::{self, Renderer, View};
use crate::session::{EventProxy, GridSize, Session, UserEvent};
use crate::window_style;

/// The starting font size in logical pixels (`Ctrl+0`); scaled by the
/// monitor's DPI.
const FONT_SIZE: f32 = 15.0;
/// Half a blink: how long the cursor stays shown, then hidden.
const BLINK_INTERVAL: Duration = Duration::from_millis(530);
/// The window's first size, in cells.
const START_GRID: GridSize = GridSize { columns: 140, lines: 40 };


/// The window and the session in it. The window, surface, font and
/// session exist once the event loop has resumed (winit creates windows
/// only then) -- `Shown`.
pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    shown: Option<Shown>,
    /// Why there's no session; its window, titled with it.
    failure: Option<(String, Option<Window>)>,
    modifiers: ModifiersState,
    /// The cell under the mouse pointer, and the button held down for
    /// drag reports.
    pointer_cell: (usize, usize),
    held_button: Option<MouseButton>,
    wheel: WheelAccumulator,
    /// Whether a blinking cursor is in its shown half, and when it flips.
    cursor_shown: bool,
    next_blink: Instant,
    /// The background and text colors the window frame was last given.
    frame_colors: Option<(Rgb, Rgb)>,
    /// The font size in logical pixels, changed by zoom.
    font_size: f32,
}

struct Shown {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    font: CellFont,
    session: Session,
    renderer: Renderer,
    grid: GridSize,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            shown: None,
            failure: None,
            modifiers: ModifiersState::empty(),
            pointer_cell: (0, 0),
            held_button: None,
            wheel: WheelAccumulator::default(),
            cursor_shown: true,
            next_blink: Instant::now() + BLINK_INTERVAL,
            frame_colors: None,
            font_size: FONT_SIZE,
        }
    }

    fn open(&self, event_loop: &ActiveEventLoop) -> Result<Shown, String> {
        let mut font = CellFont::new(FONT_SIZE);
        let start = LogicalSize::new(START_GRID.columns as u32 * font.cell_width, START_GRID.lines as u32 * font.cell_height);
        let attributes = window_style::themed(Window::default_attributes().with_title("litastum").with_inner_size(start));
        let window = Rc::new(event_loop.create_window(attributes).map_err(|err| err.to_string())?);
        // Committed IME text (Win+. emoji, CJK input) arrives as `Ime`.
        window.set_ime_allowed(true);

        font.set_size(FONT_SIZE * window.scale_factor() as f32);
        let context = softbuffer::Context::new(window.clone()).map_err(|err| err.to_string())?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|err| err.to_string())?;
        let grid = grid_for(window.inner_size(), &font);

        let program = crate::console_litastum().ok_or("litastum's console program wasn't found next to this one")?;
        let working_directory = std::env::current_dir().unwrap_or_default();
        let session = Session::spawn(&program, &working_directory, grid, (font.cell_width, font.cell_height), EventProxy(self.proxy.clone()))
            .map_err(|err| format!("couldn't start {}: {err}", program.display()))?;
        Ok(Shown { window, surface, font, session, renderer: Renderer::new(), grid })
    }

    fn mode(&self) -> TermMode {
        self.shown.as_ref().map_or(TermMode::empty(), |shown| *shown.session.term.lock().mode())
    }

    fn write(&self, bytes: impl Into<Vec<u8>>) {
        if let Some(shown) = &self.shown {
            shown.session.write(bytes.into());
        }
    }

    fn request_redraw(&self) {
        if let Some(shown) = &self.shown {
            shown.window.request_redraw();
        }
    }

    /// A new window size; `cell_changed` after a DPI change, when the
    /// grid can stay the same while every cell's pixel size doesn't.
    fn resized(&mut self, size: PhysicalSize<u32>, cell_changed: bool) {
        let Some(shown) = &mut self.shown else {
            return;
        };
        let grid = grid_for(size, &shown.font);
        if grid != shown.grid || cell_changed {
            shown.grid = grid;
            shown.session.resize(grid, (shown.font.cell_width, shown.font.cell_height));
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
        {
            let mut term = shown.session.term.lock();
            let cursor_visible = cursor_shown || !term.cursor_style().blinking;
            let origin = render::grid_origin(size.width, shown.grid.columns, shown.font.cell_width);
            let view = View { width: size.width, height: size.height, origin, cursor_visible };
            let mut images = shown.session.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let pixels = shown.renderer.draw(&mut term, &mut shown.font, &mut images, view);
            if buffer.len() == pixels.len() {
                buffer.copy_from_slice(pixels);
            }
        }
        let _ = buffer.present();
    }

    /// A key press or release. `Ctrl+V`/`Shift+Insert` paste the
    /// clipboard as typed text, as Windows Terminal does: the key itself
    /// never reaches litastum, which reads the clipboard on that key
    /// (`windows_terminal::paste_hotkey`) and swallows the typed copy.
    fn key(&mut self, event: winit::event::KeyEvent) {
        let pressed = event.state == ElementState::Pressed;
        if pressed {
            // Typing keeps the cursor steady, as in other terminals.
            self.cursor_shown = true;
            self.next_blink = Instant::now() + BLINK_INTERVAL;
        }
        if input::is_fullscreen_key(event.physical_key, self.modifiers) {
            if pressed && !event.repeat {
                self.toggle_fullscreen();
            }
            return;
        }
        if let Some(zoom) = input::zoom_chord(event.physical_key, self.modifiers) {
            if pressed {
                self.zoom(zoom);
            }
            return;
        }
        if input::is_paste_chord(&event.logical_key, event.physical_key, self.modifiers) {
            if pressed {
                self.paste();
            }
            return;
        }
        if pressed && !self.modifiers.control_key() && input::needs_plain_text(event.text.as_deref()) {
            self.write(event.text.as_deref().unwrap_or_default());
            return;
        }
        #[cfg(windows)]
        {
            use winit::platform::scancode::PhysicalKeyExtScancode;
            let scan_code = event.physical_key.to_scancode().map_or(0, |code| (code & 0xFF) as u16);
            if let Some(record) = crate::win32_input::key_record(&event.logical_key, event.physical_key, event.text.as_deref(), self.modifiers, pressed, scan_code) {
                self.write(record.sequence());
            }
        }
        #[cfg(not(windows))]
        if pressed {
            let app_cursor = self.mode().contains(TermMode::APP_CURSOR);
            if let Some(bytes) = crate::keys::encode(&event.logical_key, event.physical_key, event.text.as_deref(), self.modifiers, app_cursor) {
                self.write(bytes);
            }
        }
    }

    /// The clipboard's text as input. Bracketed on Unix when the program
    /// asked for it; ConPTY has no use for the brackets.
    fn paste(&self) {
        let Ok(text) = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) else {
            return;
        };
        let bracketed = cfg!(not(windows)) && self.mode().contains(TermMode::BRACKETED_PASTE);
        self.write(input::paste_bytes(&text, bracketed));
    }

    fn mouse(&self, action: MouseAction) {
        let (column, line) = self.pointer_cell;
        if let Some(bytes) = mouse::report(action, column, line, self.modifiers, self.mode()) {
            self.write(bytes);
        }
    }

    fn pointer_moved(&mut self, x: f64, y: f64) {
        let Some(shown) = &self.shown else {
            return;
        };
        let (origin_x, origin_y) = render::grid_origin(shown.window.inner_size().width, shown.grid.columns, shown.font.cell_width);
        let column = ((x - f64::from(origin_x)).max(0.0) as u32 / shown.font.cell_width) as usize;
        let line = ((y - f64::from(origin_y)).max(0.0) as u32 / shown.font.cell_height) as usize;
        let cell = (column.min(shown.grid.columns - 1), line.min(shown.grid.lines - 1));
        if cell != self.pointer_cell {
            self.pointer_cell = cell;
            self.mouse(MouseAction::Move { held: self.held_button });
        }
    }

    /// Mouse reports when the program takes them, else arrow keys on the
    /// alternate screen (`input::alternate_scroll`).
    fn wheel(&mut self, delta: MouseScrollDelta) {
        let cell_height = self.shown.as_ref().map_or(16, |shown| shown.font.cell_height);
        let lines = self.wheel.lines(delta, cell_height);
        if self.modifiers.control_key() {
            let zoom = if lines > 0 { Zoom::In } else { Zoom::Out };
            for _ in 0..lines.unsigned_abs() {
                self.zoom(zoom);
            }
            return;
        }
        let mode = self.mode();
        if let Some(arrows) = input::alternate_scroll(lines, mode) {
            self.write(arrows);
            return;
        }
        let action = if lines > 0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
        for _ in 0..lines.unsigned_abs() {
            self.mouse(action);
        }
    }

    /// The window frame in the program's colors (`window_style`), when
    /// they changed.
    fn sync_frame_colors(&mut self) {
        let Some(shown) = &self.shown else {
            return;
        };
        let wanted = {
            let term = shown.session.term.lock();
            let colors = term.colors();
            (colors::resolve(TermColor::Named(NamedColor::Background), colors), colors::resolve(TermColor::Named(NamedColor::Foreground), colors))
        };
        if self.frame_colors != Some(wanted) {
            self.frame_colors = Some(wanted);
            window_style::set_frame_colors(&shown.window, wanted.0, wanted.1);
        }
    }

    fn cursor_blinks(&self) -> bool {
        self.shown.as_ref().is_some_and(|shown| shown.session.term.lock().cursor_style().blinking)
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
    /// the new grid size as for any resize.
    fn zoom(&mut self, zoom: Zoom) {
        self.font_size = input::zoomed(self.font_size, zoom, FONT_SIZE);
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
            Ok(shown) => self.shown = Some(shown),
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
            WindowEvent::Focused(focused) => {
                if let Some(report) = input::focus_report(focused, self.mode()) {
                    self.write(report);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.pointer_moved(position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => {
                let action = if state == ElementState::Pressed {
                    self.held_button = Some(button);
                    MouseAction::Press(button)
                } else {
                    self.held_button = None;
                    MouseAction::Release(button)
                };
                self.mouse(action);
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
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
        if self.cursor_blinks() {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_blink));
        } else {
            self.cursor_shown = true;
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::Term(event) = event;
        match event {
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                self.sync_frame_colors();
                self.request_redraw();
            }
            Event::Title(title) => {
                if let Some(shown) = &self.shown {
                    shown.window.set_title(&title);
                }
            }
            Event::ResetTitle => {
                if let Some(shown) = &self.shown {
                    shown.window.set_title("litastum");
                }
            }
            // Replies the terminal owes the program (cursor position and
            // the like).
            Event::PtyWrite(text) => self.write(text),
            // `CSI 14 t`: the text area's size in pixels.
            Event::TextAreaSizeRequest(format) => {
                if let Some(size) = self.text_area_size() {
                    self.write(format(size));
                }
            }
            // `OSC 10/11/12 ; ?`: a color's current value.
            Event::ColorRequest(index, format) => {
                if let Some(shown) = &self.shown {
                    let color = colors::resolve_index(index, shown.session.term.lock().colors());
                    self.write(format(color));
                }
            }
            Event::Exit | Event::ChildExit(_) => event_loop.exit(),
            _ => {}
        }
    }
}


/// The window's size in whole cells, at least one of each.
fn grid_for(size: PhysicalSize<u32>, font: &CellFont) -> GridSize {
    GridSize { columns: (size.width / font.cell_width).max(1) as usize, lines: (size.height / font.cell_height).max(1) as usize }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_whole_cells_and_never_empty() {
        let font = CellFont::new(16.0);
        let size = PhysicalSize::new(font.cell_width * 10 + font.cell_width / 2, font.cell_height * 3);
        assert_eq!(grid_for(size, &font), GridSize { columns: 10, lines: 3 });
        assert_eq!(grid_for(PhysicalSize::new(1, 1), &font), GridSize { columns: 1, lines: 1 });
    }
}
