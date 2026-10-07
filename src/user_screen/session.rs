use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, EventedReadWrite};
use alacritty_terminal::vte::ansi::Processor;
use ratatui::text::Line;

use super::grid;

/// Lines a running command keeps scrolled off its screen.
const SCROLLBACK: usize = 10_000;

/// Shortest and longest wait for output: short right after activity,
/// backing off while it's quiet. Input wakes the thread at once.
const POLL_MIN: Duration = Duration::from_millis(1);
const POLL_MAX: Duration = Duration::from_millis(16);

/// After the program exits, its last output can still be on the way --
/// ConPTY draws asynchronously. Read until it's been quiet this long...
const QUIET_AFTER_EXIT: Duration = Duration::from_millis(60);
/// ...but no longer than this.
const MAX_DRAIN_AFTER_EXIT: Duration = Duration::from_secs(1);


/// A grid size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GridSize {
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


enum Message {
    Input(Vec<u8>),
    Resize(WindowSize),
}


/// Hands the program's answers to its own queries (cursor position,
/// colors) back to it.
#[derive(Clone)]
struct Listener(Sender<Message>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let _ = self.0.send(Message::Input(text.into_bytes()));
        }
    }
}


/// What a running command shows now: its lines (the tail of what it
/// printed, or its whole screen for a full-screen program), and the
/// cursor among them while the program shows one.
pub struct LiveView {
    pub lines: Vec<Line<'static>>,
    pub cursor: Option<(usize, u16)>,
    /// A full-screen program (an editor, a pager) owns the screen: the
    /// previous output isn't shown under it.
    pub full_screen: bool,
}


/// One command running in a pseudoconsole (ConPTY on Windows), its
/// output parsed into a terminal grid by `alacritty_terminal` on a thread
/// of its own -- the same engine litastum's window uses (`gui/`).
pub struct LiveCommand {
    term: Arc<FairMutex<Term<Listener>>>,
    sender: Sender<Message>,
    finished: Arc<AtomicBool>,
    changed: Arc<AtomicBool>,
    /// Set when this is dropped before the program ended (an error in the
    /// run loop): the I/O thread stops and closes the pseudoconsole, which
    /// ends the program -- neither outlives it unseen.
    abandoned: Arc<AtomicBool>,
}

impl Drop for LiveCommand {
    fn drop(&mut self) {
        self.abandoned.store(true, Ordering::Release);
    }
}

impl LiveCommand {
    /// Starts `program` with `args`, appended to its command line as they
    /// are (no quoting added: `cmd /C` and PowerShell parse the rest of
    /// the line themselves), in `cwd`, on a `columns` x `lines` screen.
    pub fn spawn(program: &str, args: Vec<String>, cwd: &Path, columns: u16, lines: u16) -> io::Result<Self> {
        let options = tty::Options {
            shell: Some(tty::Shell::new(program.to_string(), args)),
            working_directory: Some(cwd.to_path_buf()),
            ..tty::Options::default()
        };
        let size = GridSize { columns: usize::from(columns.max(1)), lines: usize::from(lines.max(1)) };
        let pty = tty::new(&options, window_size(size), 0)?;
        let (sender, receiver) = mpsc::channel();
        let config = Config { scrolling_history: SCROLLBACK, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, Listener(sender.clone()))));
        let finished = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(AtomicBool::new(true));
        let abandoned = Arc::new(AtomicBool::new(false));
        let io = Io { term: term.clone(), finished: finished.clone(), changed: changed.clone(), abandoned: abandoned.clone(), parser: Processor::new() };
        std::thread::Builder::new().name("command-io".into()).spawn(move || io.run(pty, receiver))?;
        Ok(Self { term, sender, finished, changed, abandoned })
    }

    /// Input for the program, as a terminal would send it.
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.sender.send(Message::Input(bytes));
    }

    /// Pasted text for the program: in bracketed-paste markers when it
    /// asked for them, so it can tell a paste from typing.
    pub fn paste(&self, text: &str) {
        let bytes = if self.term.lock().mode().contains(TermMode::BRACKETED_PASTE) { format!("\x1b[200~{text}\x1b[201~") } else { text.to_string() };
        self.write(bytes.into_bytes());
    }

    pub fn resize(&self, columns: u16, lines: u16) {
        let size = GridSize { columns: usize::from(columns.max(1)), lines: usize::from(lines.max(1)) };
        let _ = self.sender.send(Message::Resize(window_size(size)));
        self.term.lock().resize(size);
    }

    /// The program exited and its last output is in.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Whether there's new output since the last call.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }

    /// Whether the program asked for application cursor keys.
    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    /// Whether a full-screen program (an editor, a pager) has the screen.
    pub fn full_screen(&self) -> bool {
        self.term.lock().mode().contains(TermMode::ALT_SCREEN)
    }

    /// The cursor's line counted from the top of the scrollback -- the
    /// same numbering as `output`'s lines.
    pub fn output_line(&self) -> usize {
        let term = self.term.lock();
        let grid = term.grid();
        grid.history_size() + grid.cursor.point.line.0.max(0) as usize
    }

    /// The cursor line's text up to the cursor -- where a program reading
    /// its input echoes it.
    pub fn text_before_cursor(&self) -> String {
        grid::text_before_cursor(&self.term.lock())
    }

    /// What to draw in `rows` rows.
    pub fn view(&self, rows: usize) -> LiveView {
        let term = self.term.lock();
        let mode = *term.mode();
        let show_cursor = mode.contains(TermMode::SHOW_CURSOR);
        if mode.contains(TermMode::ALT_SCREEN) {
            let point = term.grid().cursor.point;
            let cursor = show_cursor.then(|| (point.line.0.max(0) as usize, point.column.0 as u16));
            return LiveView { lines: grid::screen_lines(&term), cursor, full_screen: true };
        }
        let (lines, cursor) = grid::tail_lines(&term, rows);
        LiveView { lines, cursor: cursor.filter(|_| show_cursor), full_screen: false }
    }

    /// Everything the finished command printed, for the user screen.
    pub fn output(&self) -> Vec<Line<'static>> {
        grid::all_lines(&self.term.lock())
    }
}


struct Io {
    term: Arc<FairMutex<Term<Listener>>>,
    finished: Arc<AtomicBool>,
    changed: Arc<AtomicBool>,
    abandoned: Arc<AtomicBool>,
    parser: Processor,
}

impl Io {
    fn run(mut self, mut pty: tty::Pty, receiver: Receiver<Message>) {
        let mut wait = POLL_MIN;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut exited: Option<Instant> = None;
        let mut last_output = Instant::now();
        loop {
            // Dropping `pty` on the way out closes the pseudoconsole.
            if self.abandoned.load(Ordering::Acquire) {
                break;
            }
            match receiver.recv_timeout(wait) {
                Ok(message) => {
                    handle(&mut pty, message);
                    wait = POLL_MIN;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(message) = receiver.try_recv() {
                handle(&mut pty, message);
            }

            let mut got_output = false;
            loop {
                match pty.reader().read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        got_output = true;
                        self.parser.advance(&mut *self.term.lock(), &buffer[..read]);
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            if got_output {
                self.changed.store(true, Ordering::Release);
                last_output = Instant::now();
                wait = POLL_MIN;
            } else {
                wait = (wait * 2).min(POLL_MAX);
            }

            if exited.is_none() && matches!(pty.next_child_event(), Some(ChildEvent::Exited(_))) {
                exited = Some(Instant::now());
            }
            if let Some(at) = exited {
                let now = Instant::now();
                if now.duration_since(last_output.max(at)) >= QUIET_AFTER_EXIT || now.duration_since(at) >= MAX_DRAIN_AFTER_EXIT {
                    break;
                }
                wait = POLL_MIN;
            }
        }
        self.finished.store(true, Ordering::Release);
        self.changed.store(true, Ordering::Release);
    }
}


fn handle(pty: &mut tty::Pty, message: Message) {
    match message {
        Message::Input(bytes) => write_all(pty, &bytes),
        Message::Resize(size) => pty.on_resize(size),
    }
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


fn window_size(size: GridSize) -> WindowSize {
    WindowSize { num_lines: size.lines as u16, num_cols: size.columns as u16, cell_width: 1, cell_height: 1 }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The whole route on a real pseudoconsole: a command's output arrives,
    /// parsed, and the command finishes.
    /// A command dropped while its program still runs (an error in the run
    /// loop) takes the program and its thread down with it.
    #[cfg(windows)]
    #[test]
    fn dropping_a_running_command_ends_it() {
        let command = LiveCommand::spawn("cmd", vec!["/C".into(), "ping -n 30 127.0.0.1".into()], &std::env::temp_dir(), 80, 24).unwrap();
        let finished = command.finished.clone();
        std::thread::sleep(Duration::from_millis(300));

        drop(command);

        let deadline = Instant::now() + Duration::from_secs(5);
        while !finished.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(finished.load(Ordering::Acquire), "the I/O thread stopped long before ping's 30 s");
    }

    #[cfg(windows)]
    #[test]
    fn a_command_runs_in_a_pseudoconsole_and_its_output_comes_back() {
        let cwd = std::env::temp_dir();
        let command = LiveCommand::spawn("cmd", vec!["/C".into(), "echo litastum-user-screen".into()], &cwd, 80, 24).unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        while !command.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(command.is_finished(), "cmd /C echo should end on its own");
        let output: Vec<String> = command.output().iter().map(|line| line.to_string()).collect();
        assert!(output.iter().any(|line| line.contains("litastum-user-screen")), "{output:?}");
    }
}
