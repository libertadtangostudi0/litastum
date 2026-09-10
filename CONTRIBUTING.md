# Contributing to litastum

Thanks for taking a look — contributions, bug reports, and even just
"this felt confusing" feedback are genuinely welcome. This project is
early and mostly solo-maintained so far, so a clear report or a small,
focused PR is often the most useful thing you can send.

## Table of contents

- [Before you start](#before-you-start)
- [Reporting a bug](#reporting-a-bug)
- [Suggesting a feature](#suggesting-a-feature)
- [Development setup](#development-setup)
- [Code style and conventions](#code-style-and-conventions)
- [Use of AI tools](#use-of-ai-tools)
- [Submitting a change](#submitting-a-change)
- [Questions](#questions)

## Before you start

Skim these first — they'll save you from duplicating work or proposing
something already decided against:

- [`README.md`](README.md) — what litastum is, current status, build
  instructions.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) — module layout and design.
- [`TODO.md`](TODO.md) — the actual punch list: known gaps, open
  questions, and things already ruled out (read a bullet fully before
  assuming something's unaddressed — many explain *why* a simpler
  approach was rejected).
- [`CLAUDE.md`](CLAUDE.md) and `.claude/rules/*.md` — the project's
  own running design log: stack choices and why, naming decisions,
  UI/theming conventions, and — often the most useful part — detailed
  postmortems of approaches that were tried and reverted. If you're
  about to suggest "why not just use X," there's a decent chance one of
  these files already covers why not.

## Reporting a bug

Open an issue with:

- What you did, what you expected, what actually happened.
- Your OS/terminal (this project targets Windows, Linux, and macOS
  terminals via `crossterm`; terminal-specific quirks are common).
- A minimal repro if you can manage one — an exact keystroke sequence
  and starting state (e.g. file/buffer contents, cursor position) is
  far more actionable than "selection sometimes breaks."
- For a rendering/highlighting bug, a screenshot helps a lot.

## Suggesting a feature

Check [`TODO.md`](TODO.md) first — it's the closest thing this project
has to a roadmap, and your idea may already be scoped (or deliberately
deferred, with a reason) there. If it's genuinely new, open an issue
describing the use case, not just the mechanism — "I want X because Y"
is easier to design around than "add a button that does X."

## Development setup

Requires a reasonably current stable Rust toolchain
([rustup](https://rustup.rs) if you don't have one).

```sh
cargo build
cargo test
cargo run
```

No other setup — no external services, no generated code, no separate
frontend build step.

## Code style and conventions

- Run `cargo fmt` before committing; keep `cargo clippy` clean for
  anything you touch.
- Split a file into a submodule directory once it passes ~500 lines —
  see existing `mod.rs` + `tests.rs` pairs (e.g. `src/editor/syntax/`)
  for the pattern this project already uses.
- Doc comments explain **why**, not just what — especially for a fix:
  name the actual bug and mechanism, not a generic "handles edge
  cases." This codebase leans heavily on doc comments as a record of
  *why* something is shaped the way it is (see any `bindings/*.rs` file
  for the level of detail expected on a non-obvious fix).
- No Cyrillic (or any non-English) in code or code comments — English
  throughout, regardless of what language an issue/PR discussion
  happens in.
- Prefer editing existing files over adding new abstractions; don't
  add speculative configurability for a case nobody's asked for yet.
- Found a bug? Add regression test coverage for it, not just the fix —
  see almost any test module in this codebase for the expected shape
  (a comment naming the real report, then an assertion that would have
  caught it).

## Use of AI tools

Reasonable use of AI tools (for writing code, fixing an issue,
implementing a feature, drafting docs, etc.) is not prohibited — this
project itself is built with heavy AI assistance, see `CLAUDE.md`. What
matters is the result, not how you got there:

- You're responsible for everything in your PR, AI-assisted or not —
  read and understand your own diff before submitting it. "The AI wrote
  it" isn't an answer to a review question.
- It still has to meet this file's own conventions above (tests for
  bugs, doc comments that explain *why*, no speculative abstractions,
  focused PRs) — AI-generated code is held to the same bar as anything
  else, not a lower one.
- Verify it actually works (`cargo test`, and try it for real) before
  opening the PR — don't submit unverified AI output on faith.

## Submitting a change

1. Fork, branch, make your change.
2. `cargo test` (and `cargo fmt`/`cargo clippy`) before opening the PR.
3. Keep the PR focused — one fix or one feature. A drive-by cleanup
   bundled into an unrelated fix makes both harder to review.
4. Describe *why*, not just what changed, in the PR description —
   same expectation as the doc-comment convention above.
5. Link the issue it addresses, if there is one.
6. Open the PR against `main` and wait for review — **all changes go
   through PR review before merging, no direct pushes to `main`,**
   maintainer's own commits included. Don't force-push over review
   feedback history without a heads-up; a follow-up commit addressing
   comments is easier to re-review than a rewritten one.

Small, well-scoped PRs get reviewed faster than large ones — if a
change is genuinely big, consider opening an issue first to align on
approach before investing the time.

## Questions

Open an issue — there's no separate chat/forum for this project yet.
