use std::io;
use std::path::Path;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use winit::event_loop::EventLoopProxy;

/// What the terminal thread tells the window thread.
#[derive(Debug)]
pub enum UserEvent {
    Term(Event),
}

/// Forwards terminal events into the window's event loop, which owns all
/// drawing.
#[derive(Clone)]
pub struct EventProxy(pub EventLoopProxy<UserEvent>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.0.send_event(UserEvent::Term(event));
    }
}


/// The grid size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub columns: usize,
    pub lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}


/// The console litastum running in a pseudoconsole (ConPTY on Windows),
/// parsed into `term` by `alacritty_terminal`'s own I/O thread.
pub struct Session {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,
}

impl Session {
    pub fn spawn(program: &Path, working_directory: &Path, size: GridSize, cell: (u32, u32), proxy: EventProxy) -> io::Result<Self> {
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.to_string_lossy().into_owned(), Vec::new())),
            working_directory: Some(working_directory.to_path_buf()),
            ..tty::Options::default()
        };
        let pty = tty::new(&options, window_size(size, cell), 0)?;
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &size, proxy.clone())));
        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();
        Ok(Self { term, notifier })
    }

    /// Input for the program, as a terminal would send it.
    pub fn write(&self, bytes: Vec<u8>) {
        self.notifier.notify(bytes);
    }

    pub fn resize(&mut self, size: GridSize, cell: (u32, u32)) {
        self.notifier.on_resize(window_size(size, cell));
        self.term.lock().resize(size);
    }
}


fn window_size(size: GridSize, cell: (u32, u32)) -> WindowSize {
    WindowSize { num_lines: size.lines as u16, num_cols: size.columns as u16, cell_width: cell.0 as u16, cell_height: cell.1 as u16 }
}
