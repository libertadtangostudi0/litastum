use serde::{Deserialize, Serialize};
use tracing::warn;

use super::parse::{MenuItem, MenuItemBody};

/// litastum's own native user-menu format -- structured (serde-backed
/// TOML) rather than Far Manager's own hand-rolled nested-block DSL
/// (`parse.rs`), specifically so this app can read-modify-write it
/// programmatically (adding/removing a menu item from the UI, planned
/// next) without a bespoke serializer for that DSL or a lossy
/// regenerate-the-whole-file step. `FarMenu.ini` itself is only ever
/// read through `parse::parse` (once, to port it -- `port_ini_to_toml`
/// below); this module never reads or writes that format.
///
/// Shape, one array-of-tables entry per item (TOML has no bare
/// top-level array, hence the `item` wrapper key):
///
/// ```toml
/// [[item]]
/// title = "status"
/// hotkey = "s"
/// commands = ["git status -s"]
///
/// [[item]]
/// title = "commit"
/// [[item.submenu]]
/// title = "Commit"
/// commands = ["git commit -m \"!?Commit title?!\""]
/// ```
///
/// `TomlMenuItem` is a separate type from `MenuItem` on purpose --
/// `MenuItem`/`MenuItemBody` are shaped for browsing/execution
/// (`state.rs`/`input.rs`), not for serde's own conventions (an enum
/// like `MenuItemBody` has no natural, readable TOML representation);
/// keeping the on-disk shape (two optional fields, `commands` xor
/// `submenu`) separate from the in-memory one (a `Commands`/`Submenu`
/// enum, always exactly one or the other) means neither has to
/// compromise for the other's sake.
#[derive(Debug, Default, Serialize, Deserialize)]
struct TomlMenuFile {
    #[serde(default)]
    item: Vec<TomlMenuItem>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlMenuItem {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    hotkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    commands: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    submenu: Option<Vec<TomlMenuItem>>,
}

impl From<&MenuItem> for TomlMenuItem {
    fn from(item: &MenuItem) -> Self {
        let hotkey = item.hotkey.map(|c| c.to_string());
        match &item.body {
            MenuItemBody::Commands(commands) => TomlMenuItem { title: item.title.clone(), hotkey, commands: Some(commands.clone()), submenu: None },
            MenuItemBody::Submenu(children) => {
                TomlMenuItem { title: item.title.clone(), hotkey, commands: None, submenu: Some(children.iter().map(TomlMenuItem::from).collect()) }
            }
        }
    }
}

impl From<TomlMenuItem> for MenuItem {
    /// A `submenu` array (even an empty one someone wrote by hand)
    /// wins over `commands` if both are somehow present -- matches the
    /// DSL parser's own "the `{` after a header always means submenu"
    /// rule in `parse.rs`, for the same reason: one item is either a
    /// submenu or a set of commands, never genuinely both at once.
    fn from(item: TomlMenuItem) -> Self {
        let hotkey = item.hotkey.and_then(|s| s.chars().next());
        let body = match item.submenu {
            Some(children) => MenuItemBody::Submenu(children.into_iter().map(MenuItem::from).collect()),
            None => MenuItemBody::Commands(item.commands.unwrap_or_default()),
        };
        MenuItem { hotkey, title: item.title, body }
    }
}


/// Parses `LitastumMenu.toml`'s own content into menu items. Malformed
/// TOML is treated the same as the DSL parser treats malformed input --
/// logged and read as empty rather than blocking `F2` outright, so one
/// broken hand-edit doesn't lock the whole feature out.
pub fn parse_toml(content: &str) -> Vec<MenuItem> {
    match toml::from_str::<TomlMenuFile>(content) {
        Ok(file) => file.item.into_iter().map(MenuItem::from).collect(),
        Err(err) => {
            warn!(%err, "LitastumMenu.toml failed to parse");
            Vec::new()
        }
    }
}

/// Serializes `items` back into `LitastumMenu.toml`'s own format.
pub fn to_toml_string(items: &[MenuItem]) -> String {
    let file = TomlMenuFile { item: items.iter().map(TomlMenuItem::from).collect() };
    // `TomlMenuFile`'s own shape (plain strings/vecs/options, no map
    // keys that could collide with TOML's reserved characters) can't
    // actually fail to serialize -- falling back to an empty file
    // rather than unwrapping keeps this infallible for callers anyway.
    toml::to_string_pretty(&file).unwrap_or_default()
}

/// Ports a real `FarMenu.ini`'s content into `LitastumMenu.toml`'s own
/// format -- parses it with the DSL parser (`parse::parse`, the same
/// one used for reading a `FarMenu.ini` directly) and serializes the
/// result as TOML. The actual value of this being a separate,
/// named function (rather than callers just chaining `parse::parse`
/// and `to_toml_string` themselves) is documenting that this is a
/// one-time conversion step, not part of the read path.
pub fn port_ini_to_toml(ini_content: &str) -> (Vec<MenuItem>, String) {
    let items = super::parse::parse(ini_content);
    let toml = to_toml_string(&items);
    (items, toml)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_flat_command_item() {
        let items = vec![MenuItem { hotkey: Some('s'), title: "status".to_string(), body: MenuItemBody::Commands(vec!["git status -s".to_string()]) }];

        let toml = to_toml_string(&items);
        let parsed = parse_toml(&toml);

        assert_eq!(parsed, items);
    }

    #[test]
    fn round_trips_a_nested_submenu() {
        let items = vec![MenuItem {
            hotkey: None,
            title: "commit".to_string(),
            body: MenuItemBody::Submenu(vec![MenuItem {
                hotkey: Some('c'),
                title: "Commit".to_string(),
                body: MenuItemBody::Commands(vec!["git commit -m \"!?Commit title?!\"".to_string()]),
            }]),
        }];

        let toml = to_toml_string(&items);
        let parsed = parse_toml(&toml);

        assert_eq!(parsed, items);
    }

    #[test]
    fn round_trips_an_item_with_no_hotkey() {
        let items = vec![MenuItem { hotkey: None, title: "log".to_string(), body: MenuItemBody::Commands(vec!["git log".to_string()]) }];

        let parsed = parse_toml(&to_toml_string(&items));

        assert_eq!(parsed[0].hotkey, None);
    }

    #[test]
    fn malformed_toml_parses_to_no_items_instead_of_panicking() {
        assert_eq!(parse_toml("not valid toml {{{"), Vec::new());
    }

    #[test]
    fn empty_file_parses_to_no_items() {
        assert_eq!(parse_toml(""), Vec::new());
    }

    #[test]
    fn port_ini_to_toml_converts_a_real_far_menu_example() {
        let ini = "s: status\ngit status -s\n\nc: commit\n{\nc: Commit\ngit commit -m \"!?Commit title?!\"\n}\n";

        let (items, toml) = port_ini_to_toml(ini);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "status");
        // The TOML round-trips back to the exact same items -- proves
        // the serialized text is actually usable, not just the
        // in-memory Vec<MenuItem> this function also returns.
        assert_eq!(parse_toml(&toml), items);
    }
}
