//! The font size per litastum screen -- the panels, Compare, the conflict
//! resolver each keep their own zoom (requested), remembered across runs
//! for the user in litastum's own `config.json` (`window_zoom`). litastum
//! names its screen with a terminal user variable (`litastum_screen`,
//! `OSC 1337 ; SetUserVar`).

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The panels' (and editor's) screen, and the zoom a screen starts with
/// before it has its own.
pub const MAIN_SCREEN: &str = "main";

/// The terminal user variable naming litastum's screen.
pub const SCREEN_VARIABLE: &str = "litastum_screen";

/// The key in `config.json` the sizes are kept under.
const CONFIG_KEY: &str = "window_zoom";


pub struct Zooms {
    /// Font size in logical pixels, by screen.
    sizes: BTreeMap<String, f32>,
    default: f32,
    /// Where they're kept; `None` in tests, which never touch the file.
    file: Option<PathBuf>,
}

impl Zooms {
    /// The saved sizes, or none (each screen then starts at `default`).
    pub fn load(default: f32) -> Self {
        let file = config_file();
        let sizes = file.as_ref().and_then(|file| read_object(file).remove(CONFIG_KEY)).and_then(|sizes| serde_json::from_value(sizes).ok()).unwrap_or_default();
        Self { sizes, default, file }
    }

    /// `screen`'s size: its own, else the main screen's, else the default.
    pub fn size_for(&self, screen: &str) -> f32 {
        self.sizes.get(screen).or_else(|| self.sizes.get(MAIN_SCREEN)).copied().unwrap_or(self.default)
    }

    /// A zoom on `screen`, saved at once into `config.json`, every other
    /// setting there left as it is (best effort: a failed write only loses
    /// it for the next run).
    pub fn set(&mut self, screen: &str, size: f32) {
        self.sizes.insert(screen.to_string(), size);
        let Some(file) = &self.file else {
            return;
        };
        let mut config = read_object(file);
        if let Ok(sizes) = serde_json::to_value(&self.sizes) {
            config.insert(CONFIG_KEY.to_string(), sizes);
        }
        if let Ok(text) = serde_json::to_string_pretty(&config) {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, text);
        }
    }

    #[cfg(test)]
    fn in_memory(default: f32) -> Self {
        Self { sizes: BTreeMap::new(), default, file: None }
    }
}


/// litastum's `config.json`: `appdata/` in the project -- no installer
/// yet, so nothing is written outside it (the installed app will use
/// `%APPDATA%\litastum\`, `app_data::app_data_dir`). `None` in tests.
fn config_file() -> Option<PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("appdata").join("config.json"))
    }
}


/// `config.json` as a plain JSON object -- every key kept, known to the
/// window or not; empty if it's missing or broken.
fn read_object(file: &std::path::Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Requested: Compare and the resolver zoom on their own.
    #[test]
    fn each_screen_keeps_its_own_zoom_and_new_ones_start_at_the_main_one() {
        let mut zooms = Zooms::in_memory(15.0);
        assert_eq!(zooms.size_for("compare"), 15.0);

        zooms.set(MAIN_SCREEN, 17.0);
        assert_eq!(zooms.size_for("compare"), 17.0, "no zoom of its own yet: the main one");
        zooms.set("compare", 12.0);

        assert_eq!(zooms.size_for("compare"), 12.0);
        assert_eq!(zooms.size_for(MAIN_SCREEN), 17.0, "unchanged by Compare's");
        assert_eq!(zooms.size_for("conflict"), 17.0);
    }

    #[test]
    fn saved_sizes_read_back() {
        let sizes: BTreeMap<String, f32> = serde_json::from_str(r#"{"main": 16.0, "compare": 11.0}"#).unwrap();
        let zooms = Zooms { sizes, default: 15.0, file: None };
        assert_eq!(zooms.size_for("compare"), 11.0);
        assert_eq!(zooms.size_for("conflict"), 16.0);
    }

    /// The zoom shares litastum's `config.json`: saving it keeps every
    /// other setting there.
    #[test]
    fn saving_keeps_the_rest_of_the_config() {
        let dir = std::env::temp_dir().join(format!("litastum-gui-zoom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.json");
        std::fs::write(&file, r#"{ "interface_theme": "dracula", "window_zoom": { "main": 16.0 } }"#).unwrap();
        let mut zooms = Zooms { sizes: BTreeMap::new(), default: 15.0, file: Some(file.clone()) };

        zooms.set("compare", 11.0);

        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written["interface_theme"], "dracula");
        assert_eq!(written["window_zoom"]["compare"], 11.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
