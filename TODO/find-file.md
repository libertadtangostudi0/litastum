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
- [x] **The "no spinner, no cancel" half of the flagged performance
      risk below is fixed; the walk's own single-threadedness is
      not.** Originally flagged, then genuinely hit once real large-tree
      use surfaced it, and finally asked directly for a head-to-head
      comparison against real Far Manager's own Find file dialog, which
      shows live progress and lets `Esc` cancel mid-search — this app's
      version used to just block with nothing on screen until an entire
      search finished, indistinguishable from a hang on a big enough
      tree even though it was only ever slow. Fixed with a new module,
      `find_file/background.rs`, mirroring `explorer::image_preview`'s
      own already-established background-thread pattern exactly (a
      one-shot `mpsc` channel, no generation counter needed since only
      one search is ever in flight at once): `spawn_search` runs
      `search::search_cancelable` (`search`'s own new sibling, taking a
      live `SearchProgress` — `AtomicUsize` visited/found counters,
      `Relaxed` throughout, read straight from the UI thread for a
      "Searching... N visited, M found" status line — and an
      `AtomicBool` checked at the same per-entry/per-candidate
      granularity `max_visited`/`max_results` already were) on a real
      background thread and returns immediately; `main.rs::wait_for_event`
      polls it every 30 ms while running (generalized from the image-
      decode-only `IMAGE_DECODE_POLL_INTERVAL` into a shared
      `BACKGROUND_TASK_POLL_INTERVAL`/`background_task_pending`/
      `poll_background_tasks`, now covering both). `FindFilePhase`
      gained a `Searching` state between `Typing` and `Results`
      (`ui/find_file.rs::draw_searching`, a terse two-line "please
      wait" screen); `Esc` during it calls `PendingSearch::cancel`
      before closing the popup, rather than just closing it and leaving
      the thread to keep churning on a result nothing's listening for
      anymore. **The walk itself is still exactly as single-threaded as
      the item below already described** — this follow-up was about
      making a slow search *visible and interruptible*, not about
      making the walk faster; a real speed fix for pure name-only
      search on a huge tree is still open, see below.
- [x] **The walk's own single-threadedness (the last piece of the
      performance risk above) is now fixed too** — asked directly, as
      an explicit next step once the item above closed the "no
      feedback, no cancel" gap but left the walk itself exactly as
      single-threaded as before. `find_file/search.rs` (704 lines, well
      past this project's own ~500-line decomposition threshold by this
      point) was split into a `search/` submodule directory
      (`code-conventions.md`) along the boundaries that were already
      implicit in it: `walk.rs` (finding candidate paths), `matching.rs`
      (does a name match a typed query/glob), `content.rs` (does a
      candidate file's own content match a "Text to find" substring),
      tied together in `mod.rs` (`SearchProgress`, `search_cancelable`).
      `walk.rs`'s own two walk functions (`matched_names`/
      `matched_files`, replacing the old single-threaded recursive
      `fs::read_dir` ones) are now built on `ignore::WalkBuilder::build_parallel`
      — the same crate, and the same walking primitive, ripgrep itself
      uses for this exact job — added as a new dependency (`ignore =
      "0.4"`, pure Rust, pulls in `crossbeam-deque`/`globset`/`bstr` and
      nothing needing a C toolchain, matching this project's own stack
      preferences). A hand-rolled work-stealing walker (a shared
      directory queue drained by a fixed thread pool, the same shape
      `content.rs::content_filter_in_parallel` already uses for its own
      flat candidate list) was considered and rejected: that shape works
      precisely because its input is a flat list known up front — a
      directory tree isn't, and subtree sizes vary wildly (a `.git`
      object store next to a single-file directory), so a hand-rolled
      equivalent would need real work-stealing logic of its own to avoid
      one thread idling while another chews through a huge subtree
      alone, which is exactly what `ignore` already provides, tested,
      as the literal reason ripgrep is fast on huge trees in the first
      place. Every one of `ignore`'s own default filters (`.gitignore`,
      hidden files, `.ignore`, git excludes) is turned off
      (`walk.rs::build_walker`, `standard_filters(false)` +
      `filter_entry`) — this app's own scope has always been "every real
      entry, minus VCS metadata directories" (`is_vcs_dir_name`), not
      gitignore-aware filtering, and turning `ignore`'s own filtering on
      would silently change *what a search finds*, not just how fast it
      runs. Both walk functions, and the content-check phase they feed,
      share the same `SearchProgress`/`AtomicBool` cancel signal the
      previous follow-up already wired up, so live progress and
      `Esc`-cancel work exactly the same as before, just genuinely
      faster now on a tree with real parallelism to exploit. Got its own
      new, lower-level test coverage (`search/walk.rs`'s own test
      module) specifically for the rewrite -- matches spread across many
      subdirectories (parallel work distribution actually reaching every
      one of them), VCS-directory pruning and root-exclusion at the walk
      level directly, and cancellation -- on top of the existing
      higher-level `search()`-driven tests (`search/mod.rs`), which
      still pass unmodified and confirm the rewrite didn't change
      observable behavior for any of them.
      **Still open, deliberately not bundled into this rewrite**: no
      default exclusion of other common noise (`target/`,
      `node_modules/`, ...) beyond VCS metadata directories -- only
      `MAX_RESULTS`/`MAX_VISITED` (`theming::config::limits()`) bound
      the damage on a tree full of it. `ignore`'s own gitignore-style
      pruning could cover this for free if ever wanted, but turning it
      on changes *what a search finds*, not just how fast it runs, so
      it's a real design question (silently skip gitignored real files,
      or ask first), not free performance -- this rewrite was scoped to
      speed, not to changing which files a search can find.
      **Two further follow-ups, asked directly** (a review pass over
      what else was worth speeding up, once the walk itself was already
      parallel):
      1. `content.rs::content_filter_in_parallel`'s work distribution
         used to split the candidate list into one static, equal-sized
         chunk per thread — starves under a real, uneven candidate list
         (a handful of huge log files mixed in among thousands of small
         source files): a thread unlucky enough to draw a chunk with the
         big files stays busy long after every other thread has run out
         of work and gone idle, the exact same shape of problem
         `search/mod.rs`'s own doc comment already gave as the reason a
         hand-rolled equal-chunking walker was rejected for the
         directory walk itself. Fixed with a shared `next_index:
         AtomicUsize` instead — every thread claims the next unclaimed
         candidate via one `fetch_add` and keeps going until the counter
         runs past the end of the list, self-balancing without needing a
         real work-stealing deque (a thread that finishes a small file
         quickly just claims another index sooner). Regression-tested
         directly (`content_filter_in_parallel_finds_every_match_across_uneven_candidates`)
         against a candidate list built specifically to be uneven (100
         small files, 5 large ones with the real match), confirming
         every match still gets found exactly once under the new
         distribution.
      2. `file_contains` used to always run the real streamed scan
         (`file_contains_with_chunk_size`) on every name-matched
         candidate, including actual binaries (`.exe`/`.dll`/images) —
         these still got read, often well past their first chunk (a
         compiled binary frequently has long valid-looking ASCII runs
         — string tables, padding — before hitting the byte that
         finally breaks UTF-8 decoding), just to reach "not a match" by
         the slow, expensive path. Fixed with `looks_binary`: a cheap
         sniff of the file's first 8000 bytes (the same convention git
         and ripgrep both use for this) checking for a null byte before
         committing to the real scan — a real binary rejects almost
         instantly instead of streaming its whole content first. Not a
         perfect classifier (a legitimate non-UTF-8 text encoding like
         UTF-16 would also trip it), but `file_contains_with_chunk_size`
         would have rejected that content anyway on the very same
         "invalid UTF-8" grounds, so nothing that would have matched
         before can start failing to match now.
- [x] **Reported directly, with a side-by-side screenshot against real
      Far Manager**: a search on a real, large tree showed "200
      results" — real Far, same tree, same mask, found 496. Root cause
      wasn't a matching bug at all: `find_file_max_results` (the
      `theming::config::limits()` cap, 200 by default) was doing exactly
      its job — bounding a huge result set — but a search that happened
      to hit that cap rendered completely indistinguishably from one
      that had genuinely finished, with no signal anywhere that
      anything had been left out. Fixed by tracking *why* a search
      stopped, not just what it found: `SearchProgress` (`search/mod.rs`)
      gained a `capped: AtomicBool`, set once by whichever phase's own
      early-exit actually fires because of `find_file_max_results`/
      `find_file_max_visited` (`walk.rs`'s two walk functions,
      `content.rs::content_filter_in_parallel`) — never set for a plain
      `Esc` cancel, which is a deliberate stop, not a surprising
      incompleteness. `FindFileState::results_capped` carries this from
      `PendingSearch::progress` into the popup once a search finishes
      (`background.rs::poll_pending_find_file_search`), and
      `ui/find_file.rs::result_count_label` turns it into a "+" on the
      result count (`"200+ results"`) — the same widely understood
      "there's more, this isn't the real total" convention a lot of
      search/notification UIs already use, rather than a misleadingly
      precise number. Raising `find_file_max_results` in `config.json`
      (already possible, just not obviously connected to this symptom
      before) is still the actual fix for wanting a bigger single
      search's worth of results; this only makes it visible that doing
      so would help.
- [x] Content search (Far's own Alt+F7 can also search *inside* files,
      via a separate "Text to find" field) — requested directly,
      explicitly asking for real Far Manager's own behavior.
      `FindFileState` gained a second field pair
      (`content_query`/`content_cursor`) and an `active_field:
      FindFileField` (`Name`/`Content`) tracking which one `Tab` and
      typed characters currently reach — same shape Far's own dialog
      has, both fields optional and AND-combined, rather than one
      combined-syntax field (e.g. `name::text`) in a single box.
      `Typing` phase now shows both label/value pairs stacked (name
      first, matching the existing field's own position), `Tab` cycles
      focus between them, and the results title folds the content query
      in too when set (`Find file: "<name>" containing "<text>"`).
      `find_file::search` takes the content substring as a third
      parameter: for each name-matched entry, a *directory* is dropped
      entirely once a content query is set (nothing inside it to search
      — Far's own dialog has the same restriction, content search only
      ever applies to files), a *file* is kept only if its own content
      also contains the substring, case-insensitively (see the streaming-
      read follow-up below for exactly how) — a file that can't be read
      as UTF-8 (binary, permission error) is silently treated as not
      matching rather than erroring the whole search out. Plain
      substring only, no
      glob/regex, no "match case"/"whole words" — Far's own dialog has
      those as separate options too, not requested here, left out of
      scope. **Reported broken almost immediately**: against a real
      tree with hundreds of thousands of files and a broad `*.*` mask,
      the popup just sat in `Typing` with no visible progress — not
      actually hung, but reading every name-matched file's own content
      one at a time, synchronously, on the same thread already blocking
      the UI, made a genuinely large search indistinguishable from a
      frozen one. Explicitly *not* fixed by capping file size or file
      count — a large real tree is this project's normal case, not
      something to search less of. Fixed by running the content check
      across a fixed thread pool (`content_filter_in_parallel`, sized to
      `std::thread::available_parallelism()`) instead of one file at a
      time — a content check is a full-file read, almost entirely I/O
      wait rather than CPU work, so many can genuinely run at once even
      on modest hardware. `search()` now walks the tree once to collect
      name-matched *files* (`collect_name_matched_files`, uncapped by
      `find_file_max_results` — only `find_file_max_visited` still bounds
      the raw walk), then content-filters that candidate list in
      parallel, stopping once `find_file_max_results` matches are found.
      Picked `std::thread::scope` + a plain chunked pool over adding
      `rayon` — this is one run-to-completion, embarrassingly-parallel
      batch with no need for work-stealing, so the extra dependency
      (already named as an acceptable direction in `search.rs`'s own
      performance-risk doc comment below) wasn't judged worth it just
      for this. Trade-off accepted: content-query results are no longer
      in walk order the way a name-only search's still are, since
      nothing in this popup's UI relied on that order to begin with. The
      name-only walk itself is untouched — this was specifically about
      the content-read cost the new field introduced, not a fix for the
      walk's own still-open single-threaded-ness (see the performance-
      risk item below, unchanged in scope by this).
      **Follow-up, asked directly for a comparison against how real Far
      Manager's own search is put together**: Far's "Text to find"
      streams a candidate file in chunks and stops at its own first
      match, rather than always reading it whole -- `file_contains`
      here used to do the opposite (`fs::read_to_string` the entire
      file, `.to_lowercase()` the entire result, *then* check) even
      after the parallelization above, wasting a full read-plus-two-
      allocations on every candidate regardless of where — or whether —
      a match actually was. Rewritten as `file_contains_with_chunk_size`
      (`CONTENT_CHUNK_SIZE` = 64 KiB, the real entry point
      `file_contains` always uses; a chunk-size *parameter* exists so
      tests can force real cross-boundary reads deterministically with
      a tiny value rather than needing a multi-hundred-KB fixture to
      exercise it by chance): streams via `std::io::Read` off a
      `BufReader`, returns `true` the moment a chunk-so-far match is
      found instead of reading to EOF regardless. Two things can
      legitimately straddle a chunk boundary and both needed explicit
      handling, not just the substring search itself: a multi-byte
      UTF-8 character (`pending_bytes` carries an incomplete trailing
      byte sequence into the next read rather than treating a mid-
      character split as invalid UTF-8) and the needle itself (`carry`
      keeps the lowercased tail of already-scanned text, trimmed back
      down to roughly the needle's own length after each check, so a
      match starting just before a boundary is still found once the
      next chunk arrives). Confirmed with dedicated tests using a 3-4
      byte chunk size specifically to force both kinds of split on
      purpose. Also closed the
      text-entry-field asymmetry
      `TODO/code-quality.md` used to note the search box hadn't
      addressed — did **not** end up threading `text_field.rs` through
      it either way (no `Ctrl+Left`/`Right` word-wise movement, no
      `Shift`-selection, no `Delete` on either field still), since that
      wasn't part of what was actually asked for this round; still an
      open, separate gap.
      **Second follow-up, requested directly**: show how long a finished
      search actually took, alongside its result count, matching real
      Far's own results dialog. `FindFileState` gained
      `search_duration: Option<Duration>`, set once in `run_search`
      (`input.rs`) by timing the `search::search` call itself
      (`Instant::now()`/`.elapsed()`) — measures the real search only,
      not popup rendering or key-handling overhead around it. Shown in
      the results title (`ui/find_file.rs::draw_results`) rather than a
      new dedicated row, so the existing fixed popup height and layout
      didn't need to change: `Find file: "<name>" — N results in
      <time>` (and still folds in the content query when set, from the
      earlier follow-up above). `format_duration` picks whole
      milliseconds under one second, two-decimal seconds at or past it
      — sub-second precision in milliseconds is what's actually useful
      for the common fast case, where a decimal-seconds value would
      mostly just show zeroes.
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
