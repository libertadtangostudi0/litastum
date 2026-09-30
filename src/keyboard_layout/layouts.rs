/// One non-Latin keyboard layout's own letter-position table --
/// `table` maps each of that layout's lowercase letters to whichever
/// Latin letter sits in the same physical key position on a standard
/// US QWERTY layout. `name` is purely documentation (shown in nothing
/// today, but names the table for whoever's reading/extending this).
pub(super) struct LayoutTable {
    #[allow(dead_code)]
    pub(super) name: &'static str,
    pub(super) table: &'static [(char, char)],
}

/// Every layout `normalize_ctrl_shortcut` checks, in order. To add one:
/// write its table below (lowercase pairs for the letter keys bindings
/// use) and list it here. Order only matters if two layouts mapped one
/// character to different letters -- the first wins.
pub(super) static LAYOUTS: &[LayoutTable] = &[LayoutTable { name: "ЙЦУКЕН (Russian)", table: RUSSIAN_JCUKEN }];

/// Standard Windows ЙЦУКЕН (Russian) layout -- confirmed directly
/// against a real report (`Ctrl+C` producing Cyrillic `с`, U+0441, not
/// Latin `c`). Covers the full letter row, not just the handful of
/// letters this app currently binds `Ctrl+<letter>` to, so adding a new
/// binding later never needs a matching table update here too.
const RUSSIAN_JCUKEN: &[(char, char)] = &[
    ('й', 'q'),
    ('ц', 'w'),
    ('у', 'e'),
    ('к', 'r'),
    ('е', 't'),
    ('н', 'y'),
    ('г', 'u'),
    ('ш', 'i'),
    ('щ', 'o'),
    ('з', 'p'),
    ('ф', 'a'),
    ('ы', 's'),
    ('в', 'd'),
    ('а', 'f'),
    ('п', 'g'),
    ('р', 'h'),
    ('о', 'j'),
    ('л', 'k'),
    ('д', 'l'),
    ('я', 'z'),
    ('ч', 'x'),
    ('с', 'c'),
    ('м', 'v'),
    ('и', 'b'),
    ('т', 'n'),
    ('ь', 'm'),
];

/// Searches every table in `LAYOUTS`, in order, for `c` (already
/// lowercase -- see `normalize_ctrl_shortcut`'s own case handling).
/// `None` means no known non-Latin layout puts a letter there, so the
/// caller leaves the key completely alone.
pub(super) fn latin_by_position(c: char) -> Option<char> {
    LAYOUTS.iter().find_map(|layout| layout.table.iter().find(|&&(from, _)| from == c).map(|&(_, to)| to))
}
