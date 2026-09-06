use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::UserDirs;

use super::state::FindFileState;

/// Finds the user's Downloads directory and hands off to
/// `write_results` — `Ctrl+S` on the results popup, requested
/// explicitly (Far Manager itself has no equivalent of this). Split
/// from `write_results` so the actual file-writing logic is testable
/// against a scratch directory rather than the real, un-injectable
/// Downloads path (same reasoning as `config.rs`'s `set_interface_theme`
/// vs. `try_persist`).
pub fn export_results(state: &FindFileState) -> io::Result<PathBuf> {
    let downloads = UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no Downloads directory available on this platform"))?;
    write_results(&downloads, state)
}

/// Writes `state.results` (one full path per line) to a new file in
/// `dir` and returns the path written. The file name embeds the search
/// query and a timestamp (`find-results_<query>_<timestamp>.txt`) so
/// repeated exports for different searches — or the same one, run
/// again later — don't overwrite each other.
fn write_results(dir: &Path, state: &FindFileState) -> io::Result<PathBuf> {
    let filename = format!("find-results_{}_{}.txt", sanitize_for_filename(&state.query), timestamp_for_filename());
    let path = dir.join(filename);

    let mut contents = String::new();
    for result in &state.results {
        contents.push_str(&result.display().to_string());
        contents.push('\n');
    }
    fs::write(&path, contents)?;
    Ok(path)
}

/// Replaces every character Windows (the strictest common case) won't
/// allow in a file name with `_`, so an arbitrary search query is
/// always safe to embed in `export_results`'s file name — falls back
/// to a fixed placeholder if that leaves nothing at all (an
/// all-wildcard query like `"***"` sanitizes to `"___"`, which is at
/// least non-empty, but an empty query itself would otherwise produce
/// a file name with two consecutive underscores and no query in it).
fn sanitize_for_filename(query: &str) -> String {
    let cleaned: String =
        query.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    if cleaned.is_empty() {
        "query".to_string()
    } else {
        cleaned
    }
}

/// `YYYY-MM-DD_HHMMSS`, computed by hand from `SystemTime` (UTC, not
/// local time — no timezone lookup this way, but this is only ever
/// used to keep export file names from colliding with each other, not
/// as a user-facing display of "when") rather than pulling in a
/// date/time crate for one file-name timestamp. The days-to-civil-date
/// conversion is Howard Hinnant's well-known public-domain algorithm
/// (<http://howardhinnant.github.io/date_algorithms.html>), not
/// something invented here.
fn timestamp_for_filename() -> String {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = since_epoch.as_secs();
    let (days, secs_of_day) = (total_secs / 86400, total_secs % 86400);
    let (hour, minute, second) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}_{hour:02}{minute:02}{second:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let day_of_era = (z - era * 146097) as u64;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::unique_scratch_dir;

    fn scratch_dir() -> PathBuf {
        unique_scratch_dir("find-file-export")
    }

    mod filename_sanitizing_tests {
        use super::*;

        #[test]
        fn sanitize_for_filename_replaces_windows_illegal_characters() {
            assert_eq!(sanitize_for_filename("*.md"), "_.md");
            assert_eq!(sanitize_for_filename("a/b\\c:d"), "a_b_c_d");
            assert_eq!(sanitize_for_filename("normal-name"), "normal-name", "nothing to change here");
        }

        #[test]
        fn sanitize_for_filename_falls_back_to_a_placeholder_when_nothing_is_left() {
            assert_eq!(sanitize_for_filename(""), "query");
        }

        #[test]
        fn sanitize_for_filename_an_all_wildcard_query_stays_non_empty() {
            // "***" sanitizes to "___" -- non-empty, so the "query"
            // fallback (for a literally empty input) doesn't kick in here.
            assert_eq!(sanitize_for_filename("***"), "___");
        }
    }

    mod date_time_tests {
        use super::*;

        #[test]
        fn civil_from_days_epoch_is_1970_01_01() {
            assert_eq!(civil_from_days(0), (1970, 1, 1));
        }

        #[test]
        fn civil_from_days_end_of_january_1970() {
            assert_eq!(civil_from_days(30), (1970, 1, 31));
            assert_eq!(civil_from_days(31), (1970, 2, 1));
        }

        #[test]
        fn civil_from_days_end_of_a_non_leap_february() {
            // 1970 is not a leap year: Jan (31, indices 0..30) + Feb (28,
            // indices 31..58) -- index 59 is the first day of March.
            assert_eq!(civil_from_days(58), (1970, 2, 28));
            assert_eq!(civil_from_days(59), (1970, 3, 1));
        }

        #[test]
        fn civil_from_days_new_year_rollovers() {
            assert_eq!(civil_from_days(365), (1971, 1, 1), "1970 has 365 days, not a leap year");
            assert_eq!(civil_from_days(365 + 365), (1972, 1, 1), "1971 also not a leap year");
        }

        #[test]
        fn timestamp_for_filename_has_the_expected_shape() {
            let timestamp = timestamp_for_filename();
            assert_eq!(timestamp.len(), 17, "YYYY-MM-DD_HHMMSS: {timestamp}");
            assert_eq!(timestamp.as_bytes()[4], b'-');
            assert_eq!(timestamp.as_bytes()[7], b'-');
            assert_eq!(timestamp.as_bytes()[10], b'_');
            assert!(
                timestamp.chars().enumerate().all(|(i, c)| [4, 7, 10].contains(&i) || c.is_ascii_digit()),
                "everything but the separators should be digits: {timestamp}"
            );
        }
    }

    mod export_tests {
        use super::*;

        #[test]
        fn write_results_writes_one_path_per_line() {
            let dir = scratch_dir();
            let mut state = FindFileState::new();
            state.query = "test".to_string();
            state.results = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];

            let path = write_results(&dir, &state).expect("export should succeed");

            assert!(path.starts_with(&dir));
            assert_eq!(fs::read_to_string(&path).unwrap(), "a.txt\nb.txt\n");
        }

        #[test]
        fn write_results_with_no_results_writes_an_empty_file() {
            let dir = scratch_dir();
            let state = FindFileState::new();

            let path = write_results(&dir, &state).expect("export should succeed even with nothing found");

            assert_eq!(fs::read_to_string(&path).unwrap(), "");
        }

        #[test]
        fn write_results_filename_embeds_the_sanitized_query() {
            let dir = scratch_dir();
            let mut state = FindFileState::new();
            state.query = "*.md".to_string();

            let path = write_results(&dir, &state).unwrap();

            let filename = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(filename.starts_with("find-results_"), "{filename}");
            assert!(filename.contains("_.md_"), "sanitized '*.md' should appear between the prefix and the timestamp: {filename}");
            assert!(filename.ends_with(".txt"), "{filename}");
        }

        // `run_export`/`Ctrl+S` dispatch itself isn't exercised end to end
        // here: `export_results` goes through the real, un-injectable
        // Downloads directory (see its own doc comment), so calling it
        // would write into the real user's Downloads folder as a side
        // effect of running the test suite. `write_results` above covers
        // everything export-related that's actually injectable.
    }
}
