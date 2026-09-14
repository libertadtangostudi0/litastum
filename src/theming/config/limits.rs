use std::sync::OnceLock;

use super::Config;

/// App-wide tunable limits/caps, with known-safe defaults baked in here
/// and overridable per-field via `config.json` (`Config`'s own new
/// fields below) -- requested directly, after an audit pass turned up
/// five unrelated hardcoded `const`s scattered across
/// `command_line/history.rs`, `explorer/find_file/search.rs`,
/// `ui/panel.rs`, `explorer/markdown_preview.rs`, and `logging.rs`, each
/// only ever fixable by editing source and rebuilding. Centralizing the
/// *values* here doesn't change who reads them -- each of those five
/// call sites still reads its own one field straight from `limits()`
/// (no new parameter threaded through any of their signatures) -- it
/// just gives every one of them the same "known default, real override
/// path" shape `interface_theme`/`popup_style`/... already have,
/// instead of a bare `const` nobody but a developer could ever change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How many typed commands `command_line/history.rs::record_history`
    /// keeps before dropping the oldest -- was `history.rs::MAX_HISTORY`.
    pub max_command_history: usize,
    /// How many matches `explorer/find_file/search.rs::search` returns
    /// at most -- was `search.rs::MAX_RESULTS`.
    pub find_file_max_results: usize,
    /// How many filesystem entries `explorer/find_file/search.rs::search`
    /// visits at most before giving up on an oversized tree -- was
    /// `search.rs::MAX_VISITED`. See that module's own doc comment for
    /// why this is millions, not thousands, now that VCS metadata
    /// directories are pruned during the walk.
    pub find_file_max_visited: usize,
    /// Narrower than this (per column), a file panel falls back to a
    /// single column -- was `ui/panel.rs::MIN_COLUMN_WIDTH`.
    pub panel_min_column_width: u16,
    /// How many lines `PageUp`/`PageDown` scroll the Markdown preview by
    /// -- was `markdown_preview.rs::PAGE_SIZE`.
    pub markdown_preview_page_size: usize,
    /// `logs/litastum.log` is truncated and restarted once a write
    /// would exceed this -- was `logging.rs::MAX_LOG_BYTES`.
    pub max_log_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_command_history: 50,
            find_file_max_results: 200,
            find_file_max_visited: 2_000_000,
            panel_min_column_width: 24,
            markdown_preview_page_size: 15,
            max_log_bytes: 30 * 1024 * 1024,
        }
    }
}

/// The resolved limits for this run -- lazily loaded once from
/// `config.json` (see `load_limits`), then cached for the rest of the
/// process. Cached rather than re-read on every call since several
/// callers are on real per-frame paths (`ui::panel::draw_panel`,
/// `MarkdownPreviewState::page_down`/`page_up`) that shouldn't touch
/// disk every redraw just to find out a value that never changes once
/// the process has started.
pub fn limits() -> &'static Limits {
    static LIMITS: OnceLock<Limits> = OnceLock::new();
    LIMITS.get_or_init(load_limits)
}

fn load_limits() -> Limits {
    #[cfg(test)]
    {
        // Same rule `config_dir()` itself follows, for the same reason:
        // a test's result must never depend on whatever config.json (or
        // `LITASTUM_CONFIG_DIR`-pointed directory) a developer running
        // the suite actually has sitting there.
        Limits::default()
    }
    #[cfg(not(test))]
    {
        let Some(config_dir) = super::config_dir() else {
            return Limits::default();
        };
        resolve_limits(&super::read_config(&config_dir))
    }
}

/// Overlays whichever fields `config` actually set onto `Limits::default()`
/// -- pulled out from `load_limits` so it's testable directly against a
/// plain in-memory `Config`, without needing a real `config.json` on
/// disk (same "injectable value, untested I/O wrapper" split this
/// module's own `config_dir()`/`read_config()` already use).
fn resolve_limits(config: &Config) -> Limits {
    let defaults = Limits::default();
    Limits {
        max_command_history: config.max_command_history.unwrap_or(defaults.max_command_history),
        find_file_max_results: config.find_file_max_results.unwrap_or(defaults.find_file_max_results),
        find_file_max_visited: config.find_file_max_visited.unwrap_or(defaults.find_file_max_visited),
        panel_min_column_width: config.panel_min_column_width.unwrap_or(defaults.panel_min_column_width),
        markdown_preview_page_size: config.markdown_preview_page_size.unwrap_or(defaults.markdown_preview_page_size),
        max_log_bytes: config.max_log_bytes.unwrap_or(defaults.max_log_bytes),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_limits_uses_defaults_when_config_sets_nothing() {
        let resolved = resolve_limits(&Config::default());
        assert_eq!(resolved, Limits::default());
    }

    #[test]
    fn resolve_limits_overlays_only_the_fields_config_actually_sets() {
        let config = Config { max_command_history: Some(500), ..Config::default() };

        let resolved = resolve_limits(&config);

        assert_eq!(resolved.max_command_history, 500);
        assert_eq!(resolved.find_file_max_results, Limits::default().find_file_max_results, "an unset field should keep its default, not zero out");
    }

    #[test]
    fn resolve_limits_overlays_every_field_independently() {
        let config = Config {
            max_command_history: Some(1),
            find_file_max_results: Some(2),
            find_file_max_visited: Some(3),
            panel_min_column_width: Some(4),
            markdown_preview_page_size: Some(5),
            max_log_bytes: Some(6),
            ..Config::default()
        };

        let resolved = resolve_limits(&config);

        assert_eq!(resolved, Limits {
            max_command_history: 1,
            find_file_max_results: 2,
            find_file_max_visited: 3,
            panel_min_column_width: 4,
            markdown_preview_page_size: 5,
            max_log_bytes: 6,
        });
    }

    /// `limits()` itself, in a test build, must stay hermetic -- same
    /// invariant `config_dir()` already guarantees, checked directly
    /// here rather than just trusted by inspection.
    #[test]
    fn limits_is_the_hardcoded_default_in_a_test_build() {
        assert_eq!(*limits(), Limits::default());
    }
}
