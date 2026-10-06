//! `cargo xtask dist [--release]`: builds litastum and lays it out in
//! `dist/` under its shipping names. On Windows the window
//! (`litastum-gui.exe`) becomes `litastum.exe` and the console app
//! (`litastum.exe`) becomes `litastum.com` -- a double click opens the
//! window, while `litastum` typed in cmd, PowerShell or Far runs the
//! console twin in place (`.COM` comes before `.EXE` in `PATHEXT`), the
//! way Visual Studio ships `devenv.exe` and `devenv.com`. A `.com` file
//! here is an ordinary executable under another extension. History:
//! docs/history/launching.md.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};


fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("dist") {
        eprintln!("usage: cargo xtask dist [--release]");
        return ExitCode::FAILURE;
    }
    let release = args.iter().any(|arg| arg == "--release");
    match dist(release) {
        Ok(dir) => {
            println!("litastum laid out in {}", dir.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("xtask dist: {err}");
            ExitCode::FAILURE
        }
    }
}


fn dist(release: bool) -> Result<PathBuf, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("no workspace root")?.to_path_buf();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    // Only the two programs: building the whole workspace would try to
    // rebuild this running xtask, which Windows refuses.
    let mut build = Command::new(cargo);
    build.current_dir(&root).args(["build", "-p", "litastum", "-p", "litastum-gui"]);
    if release {
        build.arg("--release");
    }
    let status = build.status().map_err(|err| format!("couldn't run cargo: {err}"))?;
    if !status.success() {
        return Err("the build failed".to_string());
    }

    let built = root.join("target").join(if release { "release" } else { "debug" });
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|err| format!("couldn't create {}: {err}", dist.display()))?;
    for (source, target) in shipping_names(cfg!(windows)) {
        std::fs::copy(built.join(source), dist.join(target)).map_err(|err| format!("couldn't copy {source} to {target}: {err} -- is litastum running from dist/?"))?;
    }
    // The bundled themes, found next to the executable.
    let themes = dist.join("themes");
    std::fs::create_dir_all(&themes).map_err(|err| format!("couldn't create {}: {err}", themes.display()))?;
    let entries = std::fs::read_dir(root.join("themes")).map_err(|err| format!("couldn't read themes/: {err}"))?;
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|extension| extension == "json") {
            std::fs::copy(entry.path(), themes.join(entry.file_name())).map_err(|err| format!("couldn't copy a theme: {err}"))?;
        }
    }
    Ok(dist)
}


/// Built file name -> shipped file name.
fn shipping_names(windows: bool) -> [(&'static str, &'static str); 2] {
    if windows {
        [("litastum-gui.exe", "litastum.exe"), ("litastum.exe", "litastum.com")]
    } else {
        [("litastum-gui", "litastum-gui"), ("litastum", "litastum")]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_windows_the_window_takes_the_exe_name_and_the_console_the_com() {
        assert_eq!(shipping_names(true), [("litastum-gui.exe", "litastum.exe"), ("litastum.exe", "litastum.com")]);
        assert_eq!(shipping_names(false), [("litastum-gui", "litastum-gui"), ("litastum", "litastum")]);
    }
}
