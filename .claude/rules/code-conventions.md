# Code conventions

## Decomposition

- Split a file into a submodule directory once it passes roughly
  ~500 lines
- Use the narrowest visibility that actually works

## Dead code and duplication

- exclude


## Comments and documentation

- Doc comments explain **why**, not just what. Where a comment
  documents a bug, name the actual bug that was found and the
  mechanism, not a generic "guards against edge cases."
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
  