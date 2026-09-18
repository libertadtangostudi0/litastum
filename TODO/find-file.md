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
- [x] **Results are now sorted — reported directly, compared side by
      side against real Far Manager's own results view on the same real
      tree**: Far groups matches by directory (with both directories and
      files sorted within each one); litastum's own results used to be
      in whatever order the parallel walk/content-check workers happened
      to finish in — genuinely unsorted (this is also the point at which
      the older "content-query results are no longer in walk order...
      nothing relied on that order" note earlier in this file, from
      before the walk itself was parallelized, stopped being accurate
      for the name-only path too — both are equally unordered post-
      parallelization now, and both are fixed by this). Fixed with one
      trailing sort in `search_cancelable` (`search/mod.rs`) — a plain
      case-insensitive sort of each result's full path string
      (`sort_by_cached_key`, so the lowercasing happens once per path,
      not once per comparison) — rather than a real "group by directory"
      structure: paths sharing a directory already share that
      directory's own prefix, so a plain path sort clusters them
      together and sorts files within each cluster, reproducing Far's
      own grouped look for free. Runs once, after the walk/content-check
      already finished, over however many results survived
      `find_file_max_results` — bounded by that cap, not by how large
      the searched tree was, so the extra cost is negligible next to the
      walk itself (confirmed directly: real side-by-side numbers were
      "Far — 14s, litastum — 9s" for 1291 results on the same tree,
      before this sort was even added; sorting a few thousand short
      strings afterward doesn't register against either number).
      Regression-tested (`search_results_are_sorted_case_insensitively_grouping_by_directory`)
      against directory names that differ only in case (`"Beta"` vs.
      `"alpha"`), confirming both the case-insensitivity and the
      directory-clustering side effect.
- [x] **Real bug, found via a direct file-by-file comparison against
      real Far Manager on the same tree**: `*.cpp` containing `"pragma"`
      — litastum found 1778 results, Far found 1783, with no `+`
      (`results_capped`) shown, so this wasn't the results cap either.
      Traced by hand to a real, specific missing file: a
      `#pragma managed(push, off)` line, in plain ASCII, inside a source
      file that *also* has genuinely non-UTF-8 bytes elsewhere (a
      legacy file with `Windows-1251`-encoded Cyrillic comments, not
      uncommon in an older, internationally-authored C++ codebase
      predating the project settling on UTF-8 throughout). Root cause:
      `content.rs`'s content scan (`file_contains_with_chunk_size`, now
      `file_contains_utf8_text`) decoded each chunk as UTF-8 and gave up
      entirely — returned "not a match" — the instant it hit a
      genuinely invalid UTF-8 byte sequence anywhere in the file, even
      when the actual match had already appeared earlier in the very
      same file, in a perfectly valid ASCII prefix. Fixed by splitting
      the scan on whether the *needle itself* is ASCII
      (`needle_lower.is_ascii()`, checked once in
      `file_contains_with_chunk_size`): a plain ASCII needle (`"pragma"`,
      a function name, `TODO`, the overwhelmingly common real case) now
      goes through a new `file_contains_ascii_bytes` — a raw-byte,
      case-folded (`to_ascii_lowercase`), sliding-window scan with no
      UTF-8 validity requirement on the file at all, since an ASCII byte
      sequence reads identically regardless of what encoding the *rest*
      of the file happens to be in (true for virtually every real 8-bit
      encoding used for source code — UTF-8, Windows-125x, ISO-8859-x,
      ...; UTF-16 is the real exception, since its interleaved null
      bytes break a contiguous ASCII match regardless of how it's
      searched, and isn't what this fix targets). A non-ASCII needle
      (searching for literal non-ASCII text) still goes through
      `file_contains_utf8_text` unchanged — real case-folding of
      non-ASCII text genuinely does need decoding, so that path's own
      "only searches within the file's own valid-UTF-8 prefix"
      limitation is accepted there, just no longer forced onto the
      ASCII case that never needed it. Regression-tested with the
      match both *before* and *after* simulated `Windows-1251` bytes in
      the same file, confirming the fix is a real full-file byte scan,
      not just "happens to work when the match sits before the first
      bad byte."
- [x] **Follow-up refactor + two more speed wins, asked directly ("continue
      refactoring and speeding up search as much as possible")**:
      `search/content.rs` (539 lines, past this project's own ~500-line
      decomposition threshold again after the ASCII-bytes/UTF-8-text
      split above) became a `content/` submodule directory —
      `content/mod.rs` (`content_filter_in_parallel`, the parallel
      orchestration) and `content/scan.rs` (the actual byte-level
      scanning: `file_contains`, `file_contains_with_chunk_size`, the
      binary sniff, `scan_ascii_bytes`/`scan_utf8_text`), mirroring
      `search/`'s own existing `walk.rs`/`matching.rs` split by concern.
      Two real, measured-by-reasoning (not literally benchmarked) speed
      fixes landed alongside the reorganization, both found by rereading
      the hot paths with "what's this doing on every single iteration"
      in mind:
      1. **One file open instead of two.** `looks_binary` used to open
         every candidate a second time, on its own, just to sniff its
         first ~8000 bytes ahead of the real scan opening the same file
         again right after. `file_contains_with_chunk_size` now opens
         `path` exactly once, reads its own first chunk (the same
         `CONTENT_CHUNK_SIZE`, 64 KiB, comfortably bigger than the old
         dedicated sniff window), sniffs *that* for a null byte, and
         then feeds it straight into the real scan as that scan's own
         first iteration instead of re-reading it. Multiplied by every
         name-matched candidate in a content-query search, this halves
         the file-open/initial-read overhead of the whole content-check
         phase.
      2. **The parallel loop's own "still room for more?" check is a
         plain atomic load now, not a `Mutex` lock.**
         `content_filter_in_parallel`'s per-candidate loop used to call
         `found.lock().unwrap().len() >= max_results` on *every single
         iteration*, meaning every worker thread took the same lock just
         to peek a length, even on the overwhelmingly common
         "this candidate didn't match" path. Switched to reading
         `progress.found` (already an `AtomicUsize`, already updated on
         every real push) instead — the hot, no-match path never touches
         the `Mutex` at all now; it's only locked on an actual match,
         which is comparatively rare and where a lock's own cost is
         negligible next to the file read that just happened.
      Both fixes keep every existing behavior and test unchanged (all
      pre-existing coverage still passes verbatim, just relocated to
      `content/scan.rs`'s own test module where it now belongs) —
      neither one is a visible feature, just less wasted work per
      candidate on a search with many name-matched files to content-check.
- [x] **Two more follow-ups from the same "keep speeding this up"
      review pass, both asked directly**:
      1. **`matches_query` (`matching.rs`) no longer allocates a
         lowered copy of every entry's own name just to check it.**
         `walk.rs`'s two walk functions used to call
         `entry.file_name().to_string_lossy().to_lowercase()` on *every
         single entry visited*, not just on a match -- a fresh
         short-lived `String` allocation per entry, on a walk that's
         otherwise parallel and I/O-bound, for millions of entries on a
         genuinely large tree. `matches_query` now owns the case-folding
         decision itself: when both the entry's own name and the
         (already-lowercased-once, per search, not per entry)
         query/mask are plain ASCII -- the overwhelming majority of real
         file names and masks -- it dispatches to a zero-allocation
         byte-level path (`contains_ascii_case_insensitive` for a plain
         substring, `glob_match_ascii` -- the same two-pointer algorithm
         `glob_match` already used, just over bytes with per-byte
         `to_ascii_lowercase()` folding instead of a `char` vector) --
         `walk.rs` itself now just passes `entry.file_name().to_string_lossy()`
         straight through, no `.to_lowercase()` call at all. A name or
         query with any non-ASCII character still falls back to the
         original, correct, allocating `to_lowercase()` + `char`-based
         `glob_match` path, unchanged. `to_string_lossy()` itself
         typically doesn't allocate either, on top of this -- it only
         needs to when the raw OS name isn't already valid Unicode,
         which a real file name virtually never is -- so the common case
         now visits an entry with no allocation on this line at all.
      2. **The directory walk's own thread count now scales with the
         real machine, not a fixed ceiling.** `ignore::WalkBuilder`, left
         at its own default, caps itself at
         `available_parallelism().min(12)` internally -- a machine with
         more than 12 real cores would never get to use all of them for
         the walk. `walk.rs::build_walker` now calls
         `.threads(available_parallelism())` explicitly, with the same
         one-thread fallback `content_filter_in_parallel` already uses
         if the OS won't report a core count -- making the walk's own
         thread pool consistent with the content-check phase's, which
         was already uncapped.
      Both regression-tested (`matching.rs`'s own new test module
      coverage for the ASCII fast path and its non-ASCII fallback;
      the existing `walk.rs`/`search/mod.rs` test suites all still pass
      unmodified, confirming neither change altered observable search
      behavior, only how much work it costs to get there).
      **Reported directly as no visible speedup after this landed** --
      confirms the earlier hypothesis that these were CPU-side micro-
      optimizations on a walk that's actually I/O-bound (syscall/disk
      cost, not string-processing cost), for a real tree the size this
      was already tested against. Followed up anyway, asked directly,
      with the one further piece from that same review pass:
      pre-parsing the glob pattern once per search instead of once per
      entry (`matching.rs::ParsedQuery`) -- `glob_match_chars`
      (formerly `glob_match`, the non-ASCII fallback the ASCII fast
      path above doesn't cover) used to `pattern.chars().collect::<Vec<char>>()`
      its own pattern fresh on every single entry it was reached for,
      even though the pattern never changes across one whole search.
      `ParsedQuery::new` builds that `Vec<char>` once, in
      `matched_names`/`matched_files` themselves, before the walk
      starts.
      **Found and fixed a real bug while building this**: the first
      version only built `pattern_chars` when the *query itself* was
      non-ASCII, reasoning (wrongly) that an ASCII query would only
      ever reach the already-covered ASCII fast path -- missed that
      `matches_query`'s ASCII/non-ASCII dispatch is decided per entry,
      by whether *that entry's own name* is ASCII too, so a perfectly
      ASCII glob query (`"*.md"`) still reaches the char-based fallback
      the moment it's checked against a non-ASCII file name, with an
      empty `pattern_chars` it was never given -- caught immediately by
      `matches_query_falls_back_correctly_for_non_ascii_names`'s own
      existing `"*.md"`-against-a-non-ASCII-name case failing. Fixed by
      building `pattern_chars` for any glob query, ASCII or not (still
      one allocation per whole search either way, not per entry).
      **Honest expectation for this specific fix**: for an ASCII query
      (the reported real case, `"*.cpp"`) checked against ASCII names
      (also the overwhelmingly common real case), `glob_match_chars` was
      never reached at all -- the ASCII fast path already handled it
      with zero pattern allocation, via a plain `&[u8]` slice, before
      this fix even existed. This closes the same gap for the rarer
      non-ASCII-name/non-ASCII-query cases only; it isn't expected to
      move the needle for the workload that prompted this whole
      "speed the search up" thread, which is very likely limited by
      real filesystem I/O (the tree lives under `W:\WorkCopies\...`) at
      this point, not by anything left to trim in the matching/glob
      code itself.
- [x] **Each of the two fields now has its own persisted history,
      requested directly, matching the shape already established
      elsewhere in the app -- mirroring the existing
      `editor::find_history`/`command_line::history` shape**:
      a new `explorer/find_file/history.rs` module (`load_history`/
      `save_history`, parameterized by a `path: &str` since this one
      module now serves *two* files rather than the sibling modules'
      one each -- `NAME_HISTORY_FILE`/`CONTENT_HISTORY_FILE`, both
      `.gitignore`d the same cwd-relative way `command_history.txt`/
      `editor_search_history.txt` already are; `record_history`, a
      no-op on an empty query and on an immediate repeat, same rules as
      both sibling modules). `App` gained
      `find_file_name_history`/`find_file_content_history: Vec<String>`,
      loaded/saved in `main.rs` outside `App::new` (same test-isolation
      reasoning the other two histories already established) and
      persisted once at clean exit, not incrementally -- `run_search`'s
      own extensive unit tests stay filesystem-free the same way
      `handle_search_key`'s already do.

      `FindFileState` gained `name_history_index`/`content_history_index:
      Option<usize>` and `name_history_up`/`_down`/`content_history_up`/
      `_down` methods -- the exact shell-`Up`-arrow mechanics
      `Editor::search_history_up`/`_down` already established (first
      press recalls the most recent past query, each further press
      steps one entry further back, `Down` walks the other way and
      clears the field once past the newest entry), just kept as two
      independent per-field instances rather than the editor's single
      shared index, since Find file already has two separate fields to
      browse instead of one search box. `Up`/`Down` were unbound during
      `Typing` before this (Find file's fields already use full
      `text_field.rs` cursor editing, unlike the editor's simpler
      append/backspace-only search box), so this is a pure addition, no
      existing binding lost. Typing or backspacing in a field resets
      *that* field's own history-browsing index, same "editing means
      fresh typing again" rule `Editor::search_push_char`/`_pop_char`
      already follow.

      History is recorded on `Enter` (`run_search`), not on `Esc` the
      way the editor's own search box does it -- Find file's `Enter` is
      a real, explicit "submit" step (unlike the editor's live-as-you-
      type box, which has no separate run to wait for), so recording
      there is the closer analogue to the command line's own "record on
      run" instead. Whichever field(s) are non-empty get recorded
      independently, so a search using only "Text to find" doesn't
      pollute the name mask's own history with an empty entry (and vice
      versa).

      **Deliberately left out, unlike the editor's own search box**: no
      ghost-text history autosuggestion (`End`-to-accept) -- `End`
      already has a real, different meaning on these fields (`text_field::move_end`,
      jump the cursor to the end of the line), so reusing it the way the
      editor's simpler append-only search box does would collide with
      existing, established behavior rather than extend it. Not asked
      for in this round either; `Up`/`Down` recall plus separate
      persistence was the actual request.
- [x] **`find_file/input.rs` (641 lines, past this project's own
      ~500-line decomposition threshold once the history feature above
      landed) became an `input/` submodule directory, requested
      directly** — split along the same boundary that was already
      structurally obvious in it: `typing.rs` (all of
      `FindFilePhase::Typing`'s own key handling, plus `run_search`,
      the one function it actually triggers) and `results.rs`
      (`FindFilePhase::Results`'s own key handling and every helper it
      calls — export, opening/navigating to a result, `F4`-to-edit).
      `mod.rs` keeps only what's genuinely shared across every phase:
      the top-level dispatch and the one truly cross-phase rule,
      `Esc`'s own cancel-then-close behavior — handled once there
      rather than duplicated into both `typing`/`results`, since every
      phase's `Esc` is identical except for `Searching`'s own extra
      `PendingSearch::cancel` step. A small test-only
      `input/test_support.rs` (`app_with_find_file`/`wait_for_search`,
      `pub(super)` so both sibling test modules can reach them) replaces
      what used to be one shared private helper pair at the top of the
      single flat file's own test module. Every existing test still
      passes, just relocated to whichever of `typing`/`results` its own
      phase now belongs to (and, in a few cases, calling
      `handle_typing_key`/`handle_results_key` directly instead of the
      top-level `handle_find_file_key`, now that those are real,
      independently testable functions rather than match arms inside
      one large one) — no observable behavior changed, only where the
      code implementing it lives.
