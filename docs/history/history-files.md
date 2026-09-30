# Where history files live (`history_dir.rs`) -- history

- **Relative to the working directory at first.** For one user that was
  inside a temp-like directory, and cleaning it deleted the whole
  command history.
- **The OS config directory** (next to `config.json`) was the first fix
  -- reverted: litastum must not read or write outside its own tree.
- **Next to the executable** (`target/debug/history/`) was the second --
  also not what was asked: the folder belongs in the project root.
- **`CARGO_MANIFEST_DIR`**, baked in at compile time, gives exactly that,
  whatever the launch directory.
