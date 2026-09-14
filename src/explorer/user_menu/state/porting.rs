use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::{BACKUP_SUFFIX, FAR_FILE_NAME, OWN_FILE_NAME};
use crate::explorer::user_menu::parse::MenuItem;
use crate::explorer::user_menu::toml_format;

/// Ports `far_path` (a real `FarMenu.ini`) into a `LitastumMenu.toml`
/// alongside it -- called only once the user has confirmed it
/// (`Mode::ConfirmPortFarMenu`). Returns the parsed items regardless of
/// whether either write below actually succeeded (best-effort
/// persistence, same "never block on a failed write" rule
/// `theming::config` already follows for theme/setup persistence).
///
/// Two backups happen here, both requested directly after "what if I
/// already have a menu and drop a new FarMenu.ini in" came up:
/// - An *existing* `LitastumMenu.toml` is renamed to
///   `LitastumMenu.toml.bak` before being overwritten -- porting
///   shouldn't silently discard a menu someone already built by hand
///   or through the UI.
/// - `FarMenu.ini` itself is renamed to `FarMenu.ini.bak` once ported --
///   unlike the very first version of this feature (which left it
///   completely untouched), it has to move out of the way now that
///   `resolve_menu` reports a `FarMenu.ini`'s mere presence every time
///   regardless of whether `LitastumMenu.toml` already exists; leaving
///   it in place would re-trigger this same prompt on every future
///   `F2`/startup check.
pub fn port_far_menu(far_path: &Path) -> Vec<MenuItem> {
    let dir = far_path.parent().unwrap_or_else(|| Path::new("."));
    let content = match read_text_file_any_encoding(far_path) {
        Ok(content) => content,
        Err(err) => {
            warn!(path = %far_path.display(), %err, "FarMenu.ini could not be read for porting");
            return Vec::new();
        }
    };

    let (items, toml) = toml_format::port_ini_to_toml(&content);
    let own = dir.join(OWN_FILE_NAME);

    if own.is_file() {
        let backup = dir.join(format!("{OWN_FILE_NAME}{BACKUP_SUFFIX}"));
        if let Err(err) = fs::rename(&own, &backup) {
            warn!(path = %backup.display(), %err, "failed to back up the existing LitastumMenu.toml before overwriting it");
        }
    }
    if let Err(err) = fs::write(&own, &toml) {
        warn!(path = %own.display(), %err, "failed to write LitastumMenu.toml after porting FarMenu.ini");
    }

    let far_backup = far_path.with_file_name(format!("{FAR_FILE_NAME}{BACKUP_SUFFIX}"));
    if let Err(err) = fs::rename(far_path, &far_backup) {
        warn!(path = %far_backup.display(), %err, "failed to move FarMenu.ini aside after porting it");
    }

    items
}


/// `N`/`Esc` on the "port FarMenu.ini?" prompt: moves `far_path` aside
/// to `FarMenu.ini.bak` without reading or converting it, purely so it
/// stops being detected (and re-prompted for) on every future `F2`/
/// startup check -- `resolve_menu` reports a `FarMenu.ini`'s mere
/// presence unconditionally, so declining still has to make it go away
/// somehow. Returns the backup path so the caller can tell the user
/// where it ended up; `None` if the rename itself failed (permissions,
/// ...), logged and left as a silent no-op otherwise -- the file simply
/// stays in place and gets offered again next time.
pub fn backup_far_menu_without_porting(far_path: &Path) -> Option<PathBuf> {
    let backup = far_path.with_file_name(format!("{FAR_FILE_NAME}{BACKUP_SUFFIX}"));
    match fs::rename(far_path, &backup) {
        Ok(()) => Some(backup),
        Err(err) => {
            warn!(path = %backup.display(), %err, "failed to back up FarMenu.ini");
            None
        }
    }
}


/// Reads `path` as text, decoding whichever of UTF-8, UTF-16LE, or
/// UTF-16BE it's actually encoded in -- reported directly: a real
/// `FarMenu.ini` (exported straight from an actual Far Manager
/// install) failed to port at all, silently producing zero items.
/// Confirmed by inspecting the raw bytes: it starts with `FF FE` (a
/// UTF-16LE byte-order mark) followed by every character null-padded
/// -- real Far Manager saves this file in UTF-16LE, not UTF-8, and a
/// plain `fs::read_to_string` (strict UTF-8) fails outright on it,
/// since two-byte-per-character text is essentially never valid UTF-8.
/// Detected by byte-order mark, the same convention every other text
/// tool uses to tell these apart, since nothing else in the file names
/// its own encoding. Falls back to plain UTF-8 (also stripping a UTF-8
/// BOM, `EF BB BF`, if present) when there's no UTF-16 BOM -- covers a
/// hand-written or already-UTF-8 `FarMenu.ini` too, not just Far
/// Manager's own default export.
fn read_text_file_any_encoding(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;

    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok(decode_utf16(rest, u16::from_le_bytes));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok(decode_utf16(rest, u16::from_be_bytes));
    }

    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

/// Pairs up `bytes` two at a time (via `from_units`, `u16::from_le_bytes`
/// or `u16::from_be_bytes`) into UTF-16 code units and decodes them --
/// lossy (`char::REPLACEMENT_CHARACTER` for anything malformed) rather
/// than failing outright, since a slightly-corrupt menu file should
/// still port whatever of it *does* decode rather than porting nothing
/// at all. A trailing odd byte (a malformed file missing its last low
/// or high byte) is simply dropped by `chunks_exact(2)`.
fn decode_utf16(bytes: &[u8], from_units: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| from_units([pair[0], pair[1]])).collect();
    String::from_utf16_lossy(&units)
}


#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::explorer::user_menu::parse::MenuItemBody;
    use crate::explorer::user_menu::state::scratch_dir;
    use crate::explorer::user_menu::state::{resolve_menu, MenuFile};

    mod port_far_menu_tests {
        use super::*;

        /// The actual point of porting: a real `FarMenu.ini` becomes a
        /// working `LitastumMenu.toml` alongside it.
        #[test]
        fn ports_a_far_menu_into_a_working_litastum_toml() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let far_content = "s: status\ngit status -s\n\nc: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n";
            fs::write(&far_path, far_content).unwrap();

            let items = port_far_menu(&far_path);

            assert_eq!(items.len(), 2);
            assert_eq!(items[0].title, "status");

            let MenuFile::Own(_, reread) = resolve_menu(&dir) else { panic!("expected the ported LitastumMenu.toml to now resolve") };
            assert_eq!(reread, items);
        }

        /// Regression coverage for the real request: once ported,
        /// `FarMenu.ini` has to move out of the way -- `resolve_menu`
        /// now reports its mere presence unconditionally, so leaving it
        /// in place (the very first version of this feature's own
        /// behavior) would re-trigger the same prompt on every future
        /// `F2`/startup check.
        #[test]
        fn moves_far_menu_ini_aside_after_porting_it() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let far_content = "s: status\ngit status -s\n";
            fs::write(&far_path, far_content).unwrap();

            port_far_menu(&far_path);

            assert!(!far_path.exists(), "FarMenu.ini should have been moved aside");
            let backup_content = fs::read_to_string(dir.join("FarMenu.ini.bak")).unwrap();
            assert_eq!(backup_content, far_content);
            assert!(matches!(resolve_menu(&dir), MenuFile::Own(_, _)), "should no longer be re-detected as a FarMenu.ini to port");
        }

        /// Regression coverage for the actual real-world report: a
        /// `FarMenu.ini` exported straight from a real Far Manager
        /// install ported to zero items -- confirmed by inspecting its
        /// raw bytes directly, it's UTF-16LE with a BOM (`FF FE`, every
        /// ASCII character null-padded), which a strict-UTF-8 read
        /// fails on outright. Built here the same way a real text
        /// editor saving "UTF-16 LE" would produce it, not by guessing
        /// at the byte layout.
        #[test]
        fn ports_a_real_utf16le_far_menu_ini() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let text = "s: status\r\ngit status -s\r\n";
            let mut bytes = vec![0xFF, 0xFE];
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            fs::write(&far_path, &bytes).unwrap();

            let items = port_far_menu(&far_path);

            assert_eq!(items.len(), 1, "should have actually parsed the UTF-16LE content, not silently produced nothing");
            assert_eq!(items[0].title, "status");
            assert_eq!(items[0].body, MenuItemBody::Commands(vec!["git status -s".to_string()]));
        }

        /// Regression coverage for the actual "what if I already have a
        /// menu" scenario this whole change was requested for: porting
        /// over an existing `LitastumMenu.toml` must not silently
        /// discard it.
        #[test]
        fn backs_up_an_existing_litastum_menu_before_overwriting_it() {
            let dir = scratch_dir();
            let own_path = dir.join(OWN_FILE_NAME);
            let old_content = "[[item]]\ntitle = \"old\"\ncommands = [\"echo old\"]\n";
            fs::write(&own_path, old_content).unwrap();
            let far_path = dir.join(FAR_FILE_NAME);
            fs::write(&far_path, "s: new\necho new\n").unwrap();

            let items = port_far_menu(&far_path);

            assert_eq!(items[0].title, "new", "the freshly ported menu should be what's active now");
            let backup_content = fs::read_to_string(dir.join("LitastumMenu.toml.bak")).unwrap();
            assert_eq!(backup_content, old_content, "the old menu should be preserved as a backup, not lost");
        }
    }

    mod backup_far_menu_without_porting_tests {
        use super::*;

        #[test]
        fn moves_far_menu_ini_to_a_backup_and_returns_its_path() {
            let dir = scratch_dir();
            let far_path = dir.join(FAR_FILE_NAME);
            let content = "s: status\ngit status -s\n";
            fs::write(&far_path, content).unwrap();

            let backup = backup_far_menu_without_porting(&far_path).unwrap();

            assert_eq!(backup, dir.join("FarMenu.ini.bak"));
            assert!(!far_path.exists());
            assert_eq!(fs::read_to_string(&backup).unwrap(), content);
        }

        #[test]
        fn does_not_touch_any_existing_litastum_menu() {
            let dir = scratch_dir();
            let own_path = dir.join(OWN_FILE_NAME);
            fs::write(&own_path, "[[item]]\ntitle = \"mine\"\ncommands = [\"echo mine\"]\n").unwrap();
            let far_path = dir.join(FAR_FILE_NAME);
            fs::write(&far_path, "s: theirs\necho theirs\n").unwrap();

            backup_far_menu_without_porting(&far_path);

            let MenuFile::Own(_, items) = resolve_menu(&dir) else { panic!("expected MenuFile::Own now that FarMenu.ini is gone") };
            assert_eq!(items[0].title, "mine");
        }
    }

    mod read_text_file_any_encoding_tests {
        use super::*;

        fn utf16_bytes(text: &str, bom: [u8; 2], to_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
            let mut bytes = bom.to_vec();
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&to_bytes(unit));
            }
            bytes
        }

        #[test]
        fn reads_plain_utf8() {
            let dir = scratch_dir();
            let path = dir.join("plain.txt");
            fs::write(&path, "hello").unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello");
        }

        #[test]
        fn strips_a_utf8_bom() {
            let dir = scratch_dir();
            let path = dir.join("bom.txt");
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(b"hello");
            fs::write(&path, &bytes).unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello");
        }

        /// The actual real-world case: real Far Manager's own default
        /// `FarMenu.ini` encoding.
        #[test]
        fn reads_utf16_le_with_bom() {
            let dir = scratch_dir();
            let path = dir.join("utf16le.ini");
            fs::write(&path, utf16_bytes("hello \u{416}", [0xFF, 0xFE], u16::to_le_bytes)).unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello \u{416}");
        }

        #[test]
        fn reads_utf16_be_with_bom() {
            let dir = scratch_dir();
            let path = dir.join("utf16be.ini");
            fs::write(&path, utf16_bytes("hello \u{416}", [0xFE, 0xFF], u16::to_be_bytes)).unwrap();
            assert_eq!(read_text_file_any_encoding(&path).unwrap(), "hello \u{416}");
        }

        #[test]
        fn errors_on_genuinely_invalid_utf8_with_no_bom() {
            let dir = scratch_dir();
            let path = dir.join("invalid.ini");
            fs::write(&path, [0xFF, 0x00, 0x01]).unwrap(); // 0xFF alone (no matching 0xFE) is invalid UTF-8
            assert!(read_text_file_any_encoding(&path).is_err());
        }
    }
}
