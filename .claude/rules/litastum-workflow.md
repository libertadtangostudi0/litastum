# litastum: workflow

- The user builds and runs tests themselves, TDD-style — don't run
  `cargo build`/`cargo test`/`cargo run` as routine; that loop belongs
  to them. Build/run only when actually needed to debug something we're
  stuck on together, or when explicitly asked for a status check.
- Stuck on something? Add temporary debug output/logging to isolate it,
  then remove it once resolved — don't leave debug scaffolding in the
  code that gets committed.
- Found a bug? Add regression test coverage for it, not just the fix.
- Favor clean-architecture / best-practices: modular, portable,
  maintainable, readable code — not cleverly-written. File size
  decomposition threshold (~500 lines) is in
  [[code-conventions]].
