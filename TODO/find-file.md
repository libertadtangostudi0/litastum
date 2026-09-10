# Find file (`find_file.rs`) — F9 → Commands → Find file, landed, gaps left

Type a filename substring or glob, `Enter` searches recursively from
the active panel's directory, `Enter` on a result moves the active
panel there with the file selected. Far Manager's own Alt+F7 — reached
through the menu *and* bound directly (`command_line.rs`), matching
real Far.

- [x] `F4` on a result opens it in the built-in editor, same as `F4`
      from the browser — does nothing for a directory result (search
      results can match directory names too) or a non-UTF-8 file, same
      as the browser's own `F4`. Closing that editor returns to the
      results popup with its results intact (`app.editor_return_to`,
      `editor_keymap::return_from_editor`) rather than dropping back to
      plain browsing — requested directly as a real follow-up gap:
      opening one result to check it shouldn't lose the rest of the
      search. Moved (not cloned) into `editor_return_to` and back, so a
      large result set isn't deep-copied just to park it there while
      editing.
- [x] `Tab` on a result performs the same directory navigation `Enter`
      does (moves the active panel there, selects the file), but leaves
      the popup open in `FindFilePhase::Results` instead of closing it
      — requested directly so further results can still be browsed
      (`Up`/`Down`, or `Tab` again on a different one) with the panel
      updating in the background each time, without reopening Find file.

- [x] Recursive substring search (`find_file::search`, case-insensitive,
      matches directory names too, not just files), capped at 200
      results / 50,000 visited entries so a huge tree (a repo's own
      `target/`, `.git/`, `node_modules/`, ...) can't hang the UI
      indefinitely — deliberately no directory exclusions beyond that
      cap, a plain substring search same as Far's own "Find file"
      starts as
- [x] Glob patterns (`*`/`?`, e.g. `*.md`) — reported as a real bug
      almost immediately: `*.md` was being searched for as the
      *literal* six-character substring `"*.md"`, which matches no
      real file name. `find_file::matches_query` now switches to
      `glob_match` (a hand-rolled, classic greedy two-pointer `*`/`?`
      matcher — no `[...]` character classes, no escaping) whenever the
      query actually contains a wildcard character; a plain query with
      neither still matches by substring, so "just type part of the
      name" keeps working without forcing `*name*` on every search
- [ ] **Flagged performance risk, not yet a reported problem**:
      `find_file/search.rs`'s recursive walk is synchronous, single-
      threaded, and runs directly on the key-handling thread — no
      spinner, no cancel, and no default exclusion of `.git`/`target`/
      `node_modules`/... (the only safety net is the `MAX_RESULTS`/
      `MAX_VISITED` visited-entry cap, which bounds the damage but
      doesn't make a big search *fast*). Fine for every repo size tried
      so far; revisit if a genuinely large tree makes this noticeably
      slow or freezes the UI — see `search.rs`'s own doc comment for
      the fix directions (ignore-pattern pruning, a background thread
      with cancel, or parallelizing the walk)
- [ ] No content search (Far's own Alt+F7 can also search *inside*
      files) — file names only
- [x] Results now scroll, and the popup has a fixed height — reported
      directly against a query that returned thousands of matches: the
      list used to render every item into the popup's own (still
      result-count-dependent) height with no scroll offset at all, so a
      selection deep into a long list was simply invisible. Same "`List`
      with no `ListState` doesn't auto-scroll" gap already hit (and
      fixed) twice before in this codebase — `Panel`'s own entry grid,
      and `ui/theme_menu.rs`'s color-scheme picker — fixed here the same
      way as the picker: a real `ListState` tracking the selected index,
      which `List` then scrolls to keep in view on its own. Follow-up,
      requested directly with a side-by-side comparison: once the list
      itself scrolls, there's no reason for the *popup* to keep growing
      with the result count either — `RESULTS_HEIGHT` is now a fixed
      `21`, matching `ui/theme_menu.rs`'s own popup height for its 8
      bundled themes exactly, rather than shrinking to a cramped box for
      a handful of results or ballooning to the full terminal height for
      thousands (`ui/find_file.rs::draw_results`).
- [x] `Ctrl+S` on the results popup exports the full list (one full
      path per line) to a new file in the user's Downloads directory
      (`directories::UserDirs::download_dir()`, already a dependency)
      — requested explicitly. File name embeds the query and a
      hand-rolled `YYYY-MM-DD_HHMMSS` timestamp
      (`find_file::timestamp_for_filename`/`civil_from_days` — Howard
      Hinnant's public-domain days-to-civil-date algorithm, computed by
      hand from `SystemTime` rather than pulling in a date/time crate
      for one file name) so repeated exports never overwrite each
      other; the query is sanitized for Windows-illegal filename
      characters first (`*`/`?` show up constantly here, being glob
      syntax). Both the finding-Downloads half and the actual
      file-writing half are split apart
      (`export_results`/`write_results`) so the latter has real test
      coverage against a scratch directory — same reasoning as
      `config.rs`'s `set_interface_theme` vs. `try_persist` split, since
      the former goes through a real, un-injectable OS path. Success or
      failure both show a message under the results list (no other
      status-bar surface exists yet)
