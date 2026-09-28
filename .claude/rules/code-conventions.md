# Code conventions

## Decomposition

- Split a file into a submodule directory once it passes roughly
  ~500 lines
- Use the narrowest visibility that actually works

## Dead code and duplication

- exclude


## Comments and documentation

- **Code comments stay short** -- the *current* why: what a function
  is for, the invariant it must keep, the one non-obvious constraint a
  reader would otherwise break (usually 1-3 lines, a few more for a
  genuinely tricky one). Name the real mechanism, not a generic
  "guards against edge cases."
- **History lives outside the code**, in `docs/history/<topic>.md`:
  the sequence of attempts, what each one broke, and why the current
  approach won -- the "tried X, reverted because Y" record that keeps
  a reverted idea from being re-attempted. The code links to it with
  one line (`History: docs/history/<topic>.md.`). Not imported into
  `CLAUDE.md`: read it when working on that area, rather than loading
  every chronicle into every session.
- **`.claude/rules/` holds current decisions and conventions only**,
  not chronicles -- a short summary plus a link to the history file.
- Commit messages keep the per-change story; a regression test with a
  descriptive name is what actually stops a reverted approach from
  coming back, so a real bug fix still gets one.
- Migrating the existing long comments to this layout happens in
  stages, area by area -- until an area is migrated, its long comments
  stay as they are.
- No Cyrillic in code or code comments — code and doc comments are
  English throughout, regardless of what language the conversation
  with the assistant is in. Casual/technical discussion in chat may be
  in Russian; code stays English.
- The assistant's own chat explanations (not code) should be written
  in Russian by default — commit messages and code/doc comments stay
  English regardless.
- Formatting already in use throughout the codebase: two blank lines
  between top-level items (functions, structs, impls) in Rust files;
  vertical (one-per-line) import lists once an import block has four or
  more items.
  