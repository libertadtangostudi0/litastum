//! litastum in a window of its own: a small terminal emulator that runs
//! the console litastum in a pseudoconsole and draws its screen --
//! `winit` for the window and input, `softbuffer` + `cosmic-text` for the
//! pixels, `alacritty_terminal` for the terminal itself. The console app
//! is unchanged and still runs in any terminal. History:
//! docs/history/launching.md.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod colors;
mod font;
mod images;
mod intercept;
mod icon;
// xterm key sequences are the Unix path; Windows speaks `win32_input`.
#[cfg_attr(windows, allow(dead_code))]
mod keys;
mod mouse;
mod render;
mod session;
#[cfg_attr(not(windows), allow(dead_code))]
mod win32_input;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event;
use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor, Rgb};
use alacritty_terminal::term::TermMode;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event::StartCause;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Icon, Theme, Window, WindowAttributes, WindowId};

use font::CellFont;
use mouse::MouseAction;
use render::Frame;
use session::{EventProxy, GridSize, Session, UserEvent};

/// Font size in logical pixels; scaled by the monitor's DPI.
const FONT_SIZE: f32 = 15.0;
/// Half a blink: how long the cursor stays shown, then hidden.
const BLINK_INTERVAL: Duration = Duration::from_millis(530);
/// The window's first size, in cells.
const START_GRID: GridSize = GridSize { columns: 140, lines: 40 };


fn main() {
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(_) => return,
    };
    let mut app = App::new(event_loop.create_proxy());
    let _ = event_loop.run_app(&mut app);
}


/// The window and the session in it. `window`/`surface` exist once the
/// event loop has resumed (winit creates windows only then).
struct App {
    proxy: EventLoopProxy<UserEvent>,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    font: Option<CellFont>,
    session: Option<Session>,
    grid: GridSize,
    modifiers: ModifiersState,
    /// The cell under the mouse pointer.
    pointer_cell: (usize, usize),
    /// The mouse button held down, for drag reports.
    held_button: Option<winit::event::MouseButton>,
    /// Whether a blinking cursor is in its shown half, and when it flips.
    cursor_shown: bool,
    next_blink: Instant,
    /// The background and text colors the window frame was last given
    /// (`sync_frame_colors`).
    frame_colors: Option<(Rgb, Rgb)>,
    /// Why there's no session, shown in the title.
    failure: Option<String>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            window: None,
            surface: None,
            font: None,
            session: None,
            grid: START_GRID,
            modifiers: ModifiersState::empty(),
            pointer_cell: (0, 0),
            held_button: None,
            failure: None,
            cursor_shown: true,
            next_blink: Instant::now() + BLINK_INTERVAL,
            frame_colors: None,
        }
    }

    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let probe = CellFont::new(FONT_SIZE);
        let start = LogicalSize::new(START_GRID.columns as u32 * probe.cell_width, START_GRID.lines as u32 * probe.cell_height);
        let attributes = themed(Window::default_attributes().with_title("litastum").with_inner_size(start));
        let window = Rc::new(event_loop.create_window(attributes).map_err(|err| err.to_string())?);

        let font = probe.with_size(FONT_SIZE * window.scale_factor() as f32);
        let context = softbuffer::Context::new(window.clone()).map_err(|err| err.to_string())?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|err| err.to_string())?;
        self.grid = grid_for(window.inner_size(), &font);

        let program = console_litastum().ok_or("litastum's console program wasn't found next to this one")?;
        let working_directory = std::env::current_dir().unwrap_or_default();
        let session = Session::spawn(&program, &working_directory, self.grid, (font.cell_width, font.cell_height), EventProxy(self.proxy.clone()))
            .map_err(|err| format!("couldn't start {}: {err}", program.display()))?;

        self.window = Some(window);
        self.surface = Some(surface);
        self.font = Some(font);
        self.session = Some(session);
        Ok(())
    }

    /// A new window size; `cell_changed` after a DPI change, when the
    /// grid can stay the same while every cell's pixel size doesn't.
    fn resized(&mut self, size: PhysicalSize<u32>, cell_changed: bool) {
        let (Some(font), Some(session)) = (&self.font, &mut self.session) else {
            return;
        };
        let grid = grid_for(size, font);
        if grid != self.grid || cell_changed {
            self.grid = grid;
            session.resize(grid, (font.cell_width, font.cell_height));
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn redraw(&mut self) {
        let (Some(window), Some(surface), Some(font), Some(session)) = (&self.window, &mut self.surface, &mut self.font, &self.session) else {
            return;
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if surface.resize(width, height).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        {
            let term = session.term.lock();
            let origin = render::grid_origin(size.width, self.grid.columns, font.cell_width);
            let cursor_visible = self.cursor_shown || !term.cursor_style().blinking;
            let mut frame = Frame { pixels: &mut buffer, width: size.width, height: size.height, origin, cursor_visible };
            let mut images = session.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            render::draw(&term, font, &mut images, &mut frame);
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
        if is_paste_chord(&event.logical_key, event.physical_key, self.modifiers) {
            if pressed {
                self.paste();
            }
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        #[cfg(windows)]
        {
            use winit::platform::scancode::PhysicalKeyExtScancode;
            let scan_code = event.physical_key.to_scancode().map_or(0, |code| (code & 0xFF) as u16);
            if let Some(record) = win32_input::key_record(&event.logical_key, event.physical_key, event.text.as_deref(), self.modifiers, pressed, scan_code) {
                session.write(record.sequence());
            }
        }
        #[cfg(not(windows))]
        if pressed {
            let app_cursor = session.term.lock().mode().contains(TermMode::APP_CURSOR);
            if let Some(bytes) = keys::encode(&event.logical_key, event.physical_key, event.text.as_deref(), self.modifiers, app_cursor) {
                session.write(bytes);
            }
        }
    }

    /// The clipboard's text as input, line breaks as Enter. Bracketed on
    /// Unix when the program asked for it; ConPTY has no use for the
    /// brackets.
    fn paste(&self) {
        let Some(session) = &self.session else {
            return;
        };
        let Ok(text) = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) else {
            return;
        };
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let bracketed = cfg!(not(windows)) && session.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let bytes = if bracketed { format!("\x1b[200~{text}\x1b[201~") } else { text };
        session.write(bytes.into_bytes());
    }

    fn mouse(&mut self, action: MouseAction) {
        let Some(session) = &self.session else {
            return;
        };
        let mode = *session.term.lock().mode();
        let (column, line) = self.pointer_cell;
        if let Some(bytes) = mouse::report(action, column, line, self.modifiers, mode) {
            session.write(bytes);
        }
    }

    fn pointer_moved(&mut self, x: f64, y: f64) {
        let (Some(font), Some(window)) = (&self.font, &self.window) else {
            return;
        };
        let (origin_x, origin_y) = render::grid_origin(window.inner_size().width, self.grid.columns, font.cell_width);
        let column = ((x - f64::from(origin_x)).max(0.0) as u32 / font.cell_width) as usize;
        let line = ((y - f64::from(origin_y)).max(0.0) as u32 / font.cell_height) as usize;
        let cell = (column.min(self.grid.columns - 1), line.min(self.grid.lines - 1));
        if cell != self.pointer_cell {
            self.pointer_cell = cell;
            self.mouse(MouseAction::Move { held: self.held_button });
        }
    }

    /// The window frame (title bar, its text, the border) in the
    /// program's background and text colors, which litastum hands over
    /// with `OSC 10`/`11` when its theme changes. Windows 11 only.
    fn sync_frame_colors(&mut self) {
        let (Some(window), Some(session)) = (&self.window, &self.session) else {
            return;
        };
        let wanted = {
            let term = session.term.lock();
            let colors = term.colors();
            (colors::resolve(TermColor::Named(NamedColor::Background), colors), colors::resolve(TermColor::Named(NamedColor::Foreground), colors))
        };
        if self.frame_colors == Some(wanted) {
            return;
        }
        self.frame_colors = Some(wanted);
        #[cfg(windows)]
        {
            use winit::platform::windows::{Color, WindowExtWindows};
            let (background, text) = wanted;
            window.set_title_background_color(Some(Color::from_rgb(background.r, background.g, background.b)));
            window.set_title_text_color(Color::from_rgb(text.r, text.g, text.b));
            window.set_border_color(Some(Color::from_rgb(background.r, background.g, background.b)));
        }
        #[cfg(not(windows))]
        let _ = window;
    }

    fn cursor_blinks(&self) -> bool {
        self.session.as_ref().is_some_and(|session| session.term.lock().cursor_style().blinking)
    }

    fn scale_changed(&mut self, scale_factor: f64) {
        if let Some(font) = self.font.take() {
            self.font = Some(font.with_size(FONT_SIZE * scale_factor as f32));
        }
        if let Some(size) = self.window.as_ref().map(|window| window.inner_size()) {
            self.resized(size, true);
        }
    }

    fn wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines.round() as i32,
            MouseScrollDelta::PixelDelta(position) => (position.y / f64::from(self.font.as_ref().map_or(16, |font| font.cell_height))).round() as i32,
        };
        let action = if lines > 0 { MouseAction::WheelUp } else { MouseAction::WheelDown };
        for _ in 0..lines.unsigned_abs() {
            self.mouse(action);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.failure.is_some() {
            return;
        }
        if let Err(failure) = self.open_window(event_loop) {
            // No console to print to: say it in a window title, then quit.
            let attributes = themed(Window::default_attributes().with_title(format!("litastum: {failure}")));
            if let Ok(window) = event_loop.create_window(attributes) {
                self.window = Some(Rc::new(window));
            }
            self.failure = Some(failure);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resized(size, false),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.scale_changed(scale_factor),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(event),
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
            if let Some(window) = &self.window {
                window.request_redraw();
            }
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
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            Event::Title(title) => {
                if let Some(window) = &self.window {
                    window.set_title(&title);
                }
            }
            Event::ResetTitle => {
                if let Some(window) = &self.window {
                    window.set_title("litastum");
                }
            }
            // Replies the terminal owes the program (device attributes,
            // cursor position) -- litastum's image-protocol query waits
            // for one at startup.
            Event::PtyWrite(text) => {
                if let Some(session) = &self.session {
                    session.write(text.into_bytes());
                }
            }
            // `CSI 14 t`: the text area's size in pixels.
            Event::TextAreaSizeRequest(format) => {
                if let (Some(session), Some(font)) = (&self.session, &self.font) {
                    let size = alacritty_terminal::event::WindowSize {
                        num_lines: self.grid.lines as u16,
                        num_cols: self.grid.columns as u16,
                        cell_width: font.cell_width as u16,
                        cell_height: font.cell_height as u16,
                    };
                    session.write(format(size).into_bytes());
                }
            }
            // `OSC 10/11/12 ; ?`: a color's current value.
            Event::ColorRequest(index, format) => {
                if let Some(session) = &self.session {
                    let color = colors::resolve_index(index, session.term.lock().colors());
                    session.write(format(color).into_bytes());
                }
            }
            Event::Exit | Event::ChildExit(_) => event_loop.exit(),
            _ => {}
        }
    }
}


/// A dark window frame in the terminal's own colors, whatever the system
/// theme: the title bar was the system's light or accent color (yellow
/// in the report) above a dark grid. The exact colors are Windows 11's;
/// elsewhere the dark theme is as far as it goes.
fn themed(attributes: WindowAttributes) -> WindowAttributes {
    let icon = Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE).ok();
    let attributes = attributes.with_theme(Some(Theme::Dark)).with_window_icon(icon.clone());
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::{Color, WindowAttributesExtWindows};
        let color = |rgb: alacritty_terminal::vte::ansi::Rgb| Color::from_rgb(rgb.r, rgb.g, rgb.b);
        attributes
            .with_taskbar_icon(icon)
            .with_title_background_color(Some(color(colors::BACKGROUND)))
            .with_title_text_color(color(colors::FOREGROUND))
            .with_border_color(Some(color(colors::BACKGROUND)))
    };
    attributes
}


/// `Ctrl+V` (by physical key, any layout) or `Shift+Insert`.
fn is_paste_chord(logical: &Key, physical: PhysicalKey, mods: ModifiersState) -> bool {
    let ctrl_v = mods.control_key() && !mods.shift_key() && !mods.alt_key() && physical == PhysicalKey::Code(KeyCode::KeyV);
    let shift_insert = mods.shift_key() && !mods.control_key() && *logical == Key::Named(NamedKey::Insert);
    ctrl_v || shift_insert
}


/// The window's size in whole cells, at least one of each.
fn grid_for(size: PhysicalSize<u32>, font: &CellFont) -> GridSize {
    GridSize { columns: (size.width / font.cell_width).max(1) as usize, lines: (size.height / font.cell_height).max(1) as usize }
}


/// The console litastum next to this program -- they ship together:
/// `litastum.com` in `dist/` (where this program is `litastum.exe`
/// itself, so never that), `litastum.exe` in `target/`.
fn console_litastum() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    let names: &[&str] = if cfg!(windows) { &["litastum.com", "litastum.exe"] } else { &["litastum"] };
    names.iter().map(|name| dir.join(name)).find(|program| program.exists() && !is_same_file(program, &me))
}


fn is_same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
