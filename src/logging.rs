use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::sync::Mutex;

use color_eyre::eyre::Result;
use tracing_subscriber::EnvFilter;

const LOG_PATH: &str = "logs/litastum.log";


/// File logging to `logs/litastum.log` -- the TUI owns the terminal.
/// Capped at `limits().max_log_bytes` (`SizeCappedFile`); reading
/// `limits()` this early is fine, it's cached after the first read. Level
/// defaults to `debug` for this crate and `info` for dependencies
/// (`.claude/rules/logging.md`); override with `RUST_LOG`.
pub fn init() -> Result<()> {
    std::fs::create_dir_all("logs")?;
    let file = SizeCappedFile::open(LOG_PATH, crate::theming::config::limits().max_log_bytes)?;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,litastum=debug"));

    tracing_subscriber::fmt()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(filter)
        .init();

    Ok(())
}


/// A log file that overwrites itself from the start once it reaches
/// `cap` bytes, instead of growing without bound. Not a true rotating
/// ring buffer (a write straddling the wrap point starts a fresh file
/// rather than splitting), which is a fine trade for a debug log:
/// simple, and the file never exceeds `cap` by more than one write.
struct SizeCappedFile {
    file: File,
    written: u64,
    cap: u64,
}

impl SizeCappedFile {
    fn open(path: &str, cap: u64) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)?;

        let existing_len = file.metadata()?.len();
        let written = if existing_len >= cap {
            file.set_len(0)?;
            0
        } else {
            existing_len
        };
        file.seek(SeekFrom::End(0))?;

        Ok(Self { file, written, cap })
    }
}

impl Write for SizeCappedFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > self.cap {
            self.file.set_len(0)?;
            self.file.seek(SeekFrom::Start(0))?;
            self.written = 0;
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A log path in a fresh, empty directory per test. A name made of the
    /// process id and a counter met a file left by an earlier run with the
    /// same id, and `writes_under_the_cap_accumulate` failed now and then.
    fn test_log_path() -> String {
        crate::test_support::unique_scratch_dir("log").join("test.log").to_string_lossy().into_owned()
    }

    #[test]
    fn writes_under_the_cap_accumulate() {
        let path = test_log_path();
        let mut file = SizeCappedFile::open(&path, 10).unwrap();

        file.write_all(b"hello").unwrap(); // 5 bytes, under cap
        file.write_all(b"world").unwrap(); // 10 bytes total, still not over

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "helloworld");
    }

    #[test]
    fn write_past_the_cap_wraps_to_the_start() {
        let path = test_log_path();
        let mut file = SizeCappedFile::open(&path, 10).unwrap();

        file.write_all(b"helloworld").unwrap(); // exactly at cap
        file.write_all(b"!").unwrap(); // would exceed cap -> wraps

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "!");
    }

    #[test]
    fn reopening_an_oversized_file_truncates_it() {
        let path = test_log_path();
        std::fs::write(&path, "this content is over the tiny cap").unwrap();

        let file = SizeCappedFile::open(&path, 10).unwrap();

        assert_eq!(file.written, 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn reopening_a_file_within_the_cap_keeps_appending() {
        let path = test_log_path();
        std::fs::write(&path, "hello").unwrap();

        let mut file = SizeCappedFile::open(&path, 10).unwrap();
        file.write_all(b"!").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello!");
    }
}
