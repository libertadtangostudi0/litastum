# Find file's search engine -- history

Code: `src/explorer/find_file/search/` (`mod.rs::search_cancelable`,
`walk.rs`, `matching.rs`, `content/`), run in the background by
`find_file/background.rs`.

## How it got here, in order

1. **Synchronous recursive `fs::read_dir` on the key thread**, with VCS
   metadata directories (`.git`/`.svn`/`.hg`/`.bzr`) pruned. The pruning
   came from a report: searching a real Subversion working copy for a
   file several levels deep found nothing -- `.svn` keeps a pristine copy
   of every versioned file, enough to exhaust the visited-entry cap
   before the walk reached the target.
2. **"Text to find"** (Far's second field, a content substring
   AND-combined with the name match). Directories drop out once it's
   set -- nothing inside to search.
3. **Content check across a thread pool** (`content_filter_in_parallel`):
   a `*.*` search over hundreds of thousands of files, reading each
   match's content one at a time on the UI thread, looked hung. Fixed by
   parallelism, deliberately not by capping file size or count -- a
   large tree was the normal case being reported.
4. **Streaming chunks, stopping at the first match**, like Far, instead
   of `fs::read_to_string` + `.to_lowercase()` (whole file, two copies,
   wherever the match was).
5. **Live progress and `Esc` to cancel**, like Far's dialog:
   `SearchProgress`, a cancel flag through every function, and a
   background thread (the `image_preview` pattern).
6. **The walk in parallel too**, via `ignore::WalkBuilder::build_parallel`
   (ripgrep's walker). A hand-rolled walker was rejected: the content
   pass chunks a flat list evenly, but subtree sizes vary wildly (a
   `.git` object store next to a one-file directory), so a tree walk
   needs real work stealing, which `ignore` already has. All of
   `ignore`'s own filters (`.gitignore`, hidden, `.ignore`) are off:
   the scope has always been "every real entry minus VCS metadata", and
   turning them on would change *what* a search finds.
   `threads(available_parallelism())` overrides `ignore`'s own cap of 12
   (requested: scale with the machine), matching the content pool.

## Reports since

- **A capped search looked complete.** A search that stopped at exactly
  `find_file_max_results` (200) looked like a full result -- Far found
  more than double on the same tree. `SearchProgress::capped` now makes
  the count show a "+". A user `Esc` never sets it.
- **Results in thread order.** Once both passes were parallel, results
  came out in whatever order threads finished. Compared against Far's
  view (grouped by directory, sorted within): a case-insensitive sort of
  the full path gives the same grouping for free, and costs at most the
  result cap.
- **The file was opened twice** per candidate: once for a separate
  binary sniff, once for the scan. Now one open, and the first chunk
  serves both (a 64 KiB sniff window, larger than the old 8000 bytes).
- **Binaries were streamed in full** just to hit an invalid byte and
  bail, often far in -- compiled binaries have long ASCII runs. A null
  byte in the first chunk now skips the file (git's and ripgrep's
  heuristic). UTF-16 is also skipped, which the UTF-8 scan would reject
  anyway.
- **Five files missing versus Far** (1778 vs 1783, not a cap). One
  missing file had `#pragma managed(push, off)` in plain ASCII and
  Windows-1251 Cyrillic comments elsewhere; the UTF-8 scan gave up at
  the first invalid byte. An ASCII needle reads the same in every
  ASCII-compatible 8-bit encoding, so it now searches raw bytes with no
  UTF-8 requirement (`scan_ascii_bytes`). A non-ASCII needle still
  needs real case folding and keeps the UTF-8 scan, with its "valid
  prefix only" limit.

## Still open

No default exclusion of other noise (`target/`, `node_modules/`, ...):
only the caps bound it. `ignore`'s gitignore pruning would cover it,
but that changes which files are found -- a design question, not free
speed.
