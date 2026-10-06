use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, EventedReadWrite};
use alacritty_terminal::vte::ansi::Processor;
use winit::event_loop::EventLoopProxy;

use crate::images::{self, PlacedImage};
use crate::intercept::Interceptor;

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


/// The environment variable telling the console litastum it runs in
/// this window, and the cell size in pixels (`"10x20"`): its image picker
/// then uses iTerm2 inline images at that size. It can't learn either by
/// asking -- ConPTY answers the status query that ends the picker's
/// questions itself, before the window can reply. Read by the app's
/// `image_host` module.
const HOST_CELL_SIZE_ENV_VAR: &str = "LITASTUM_HOST_CELL_SIZE";


enum Message {
    Input(Vec<u8>),
    Resize(WindowSize),
}


/// The console litastum running in a pseudoconsole (ConPTY on Windows).
/// Our own I/O thread reads it, rather than `alacritty_terminal`'s event
/// loop: iTerm2 inline images (`OSC 1337`, the one graphics protocol
/// ConPTY passes through) must be cut out of the stream before parsing,
/// at the cursor position they arrive at.
pub struct Session {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    pub images: Arc<Mutex<Vec<PlacedImage>>>,
    sender: Sender<Message>,
}

impl Session {
    pub fn spawn(program: &Path, working_directory: &Path, size: GridSize, cell: (u32, u32), proxy: EventProxy) -> io::Result<Self> {
        let mut options = tty::Options {
            shell: Some(tty::Shell::new(program.to_string_lossy().into_owned(), Vec::new())),
            working_directory: Some(working_directory.to_path_buf()),
            ..tty::Options::default()
        };
        options.env.insert(HOST_CELL_SIZE_ENV_VAR.to_string(), format!("{}x{}", cell.0, cell.1));
        let pty = tty::new(&options, window_size(size, cell), 0)?;
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &size, proxy.clone())));
        let images = Arc::new(Mutex::new(Vec::new()));
        let (sender, receiver) = mpsc::channel();
        let io = Io { term: term.clone(), images: images.clone(), proxy, parser: Processor::new(), interceptor: Interceptor::default() };
        std::thread::Builder::new().name("pty-io".into()).spawn(move || io.run(pty, receiver))?;
        Ok(Self { term, images, sender })
    }

    /// Input for the program, as a terminal would send it.
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.sender.send(Message::Input(bytes));
    }

    pub fn resize(&mut self, size: GridSize, cell: (u32, u32)) {
        let _ = self.sender.send(Message::Resize(window_size(size, cell)));
        self.term.lock().resize(size);
        // Images are placed on cells; a new grid moves everything.
        self.images.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    }
}


/// Shortest and longest wait for the program's output: short right after
/// activity, backing off while it's quiet. Input wakes it at once.
const POLL_MIN: Duration = Duration::from_millis(1);
const POLL_MAX: Duration = Duration::from_millis(16);


struct Io {
    term: Arc<FairMutex<Term<EventProxy>>>,
    images: Arc<Mutex<Vec<PlacedImage>>>,
    proxy: EventProxy,
    parser: Processor,
    interceptor: Interceptor,
}

impl Io {
    fn run(mut self, mut pty: tty::Pty, receiver: Receiver<Message>) {
        let mut wait = POLL_MIN;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match receiver.recv_timeout(wait) {
                Ok(message) => {
                    self.handle(&mut pty, message);
                    // The program answers input: look again soon, not after
                    // the quiet-time backoff (up to 16 ms of lag per key).
                    wait = POLL_MIN;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(message) = receiver.try_recv() {
                self.handle(&mut pty, message);
            }

            let mut got_output = false;
            loop {
                match pty.reader().read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        got_output = true;
                        self.output(&buffer[..read]);
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            if got_output {
                self.proxy.send_event(Event::Wakeup);
                wait = POLL_MIN;
            } else {
                wait = backoff(wait);
            }

            if let Some(ChildEvent::Exited(status)) = pty.next_child_event() {
                self.proxy.send_event(status.map_or(Event::Exit, Event::ChildExit));
                return;
            }
        }
    }

    fn handle(&mut self, pty: &mut tty::Pty, message: Message) {
        match message {
            Message::Input(bytes) => write_all(pty, &bytes),
            Message::Resize(size) => pty.on_resize(size),
        }
    }

    /// One read: text to the terminal engine, inline images onto the
    /// grid at the cursor cell they arrive at. The term is locked once per
    /// read, not per piece.
    fn output(&mut self, bytes: &[u8]) {
        let Self { term, images, parser, interceptor, .. } = self;
        let term = std::cell::RefCell::new(term.lock());
        interceptor.feed(
            bytes,
            |text| parser.advance(&mut **term.borrow_mut(), text),
            |body| {
                let cursor = term.borrow().grid().cursor.point;
                if let Some(image) = images::decode(&body, cursor.line.0.max(0) as usize, cursor.column.0) {
                    let mut images = images.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    // A new image where another was replaces it.
                    images.retain(|old| (old.line, old.column) != (image.line, image.column));
                    images.push(image);
                }
            },
        );
    }
}


/// The next wait while the program is quiet: doubling up to `POLL_MAX`.
fn backoff(wait: Duration) -> Duration {
    (wait * 2).min(POLL_MAX)
}


/// Writes all of `bytes`, waiting out a full pipe (the writer doesn't
/// block).
fn write_all(pty: &mut tty::Pty, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match pty.writer().write(bytes) {
            Ok(0) => return,
            Ok(written) => bytes = &bytes[written..],
            Err(err) if matches!(err.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => std::thread::sleep(POLL_MIN),
            Err(_) => return,
        }
    }
    let _ = pty.writer().flush();
}


fn window_size(size: GridSize, cell: (u32, u32)) -> WindowSize {
    WindowSize { num_lines: size.lines as u16, num_cols: size.columns as u16, cell_width: cell.0 as u16, cell_height: cell.1 as u16 }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_program_is_polled_less_often_up_to_a_cap() {
        assert_eq!(backoff(POLL_MIN), POLL_MIN * 2);
        assert_eq!(backoff(POLL_MAX), POLL_MAX);
        assert_eq!(backoff(Duration::from_millis(12)), POLL_MAX);
    }
}
