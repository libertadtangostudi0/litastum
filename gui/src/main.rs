//! litastum in a window of its own: a small terminal emulator that runs
//! the console litastum in a pseudoconsole and draws its screen --
//! `winit` for the window and input, `softbuffer` + `cosmic-text` for the
//! pixels, `alacritty_terminal` for the terminal itself. The console app
//! is unchanged and still runs in any terminal. History:
//! docs/history/launching.md.
//! - `app`: the window, its events, the session in it;
//! - `session`: the pseudoconsole and its I/O thread; `intercept`/`images`
//!   cut iTerm2 inline images out of its output;
//! - `render`: the grid into pixels, only what changed; `font`, `colors`;
//! - `input`, `win32_input`, `keys`, `mouse`: what the program is sent;
//! - `window_style`, `icon`: the frame's colors and the icon.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod colors;
mod font;
mod icon;
mod images;
mod input;
mod intercept;
// xterm key sequences are the Unix path; Windows speaks `win32_input`.
#[cfg_attr(windows, allow(dead_code))]
mod keys;
mod mouse;
mod render;
mod session;
#[cfg_attr(not(windows), allow(dead_code))]
mod win32_input;
mod window_style;

use std::path::{Path, PathBuf};

use winit::event_loop::EventLoop;

use session::UserEvent;


fn main() {
    let Ok(event_loop) = EventLoop::<UserEvent>::with_user_event().build() else {
        return;
    };
    let mut app = app::App::new(event_loop.create_proxy());
    let _ = event_loop.run_app(&mut app);
}


/// The console litastum next to this program -- they ship together:
/// `litastum.com` in `dist/` (where this program is `litastum.exe`
/// itself, so never that), `litastum.exe` in `target/`.
fn console_litastum() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    console_litastum_beside(&me)
}


fn console_litastum_beside(me: &Path) -> Option<PathBuf> {
    let dir = me.parent()?;
    let names: &[&str] = if cfg!(windows) { &["litastum.com", "litastum.exe"] } else { &["litastum"] };
    names.iter().map(|name| dir.join(name)).find(|program| program.is_file() && !is_same_file(program, me))
}


fn is_same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("litastum-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// In `dist/` this program is `litastum.exe` itself: the console twin
    /// is `litastum.com`, and it must never start itself.
    #[cfg(windows)]
    #[test]
    fn the_console_twin_is_found_and_never_this_program() {
        let dist = scratch("dist");
        std::fs::write(dist.join("litastum.exe"), b"").unwrap();
        assert_eq!(console_litastum_beside(&dist.join("litastum.exe")), None, "only itself there");

        std::fs::write(dist.join("litastum.com"), b"").unwrap();
        assert_eq!(console_litastum_beside(&dist.join("litastum.exe")), Some(dist.join("litastum.com")));

        let target = scratch("target");
        std::fs::write(target.join("litastum.exe"), b"").unwrap();
        assert_eq!(console_litastum_beside(&target.join("litastum-gui.exe")), Some(target.join("litastum.exe")));
    }
}
