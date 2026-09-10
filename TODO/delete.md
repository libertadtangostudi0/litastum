# Delete (`F8`) — landed, gaps left

Confirm-before-delete, same shape as the editor's `ConfirmDiscard`
prompt: `F8` opens `Mode::ConfirmDelete(PendingDelete)` instead of
deleting immediately, `Y` deletes (`fs::remove_file` for a file,
`fs::remove_dir_all` for a directory — recurses without asking twice,
matching Far Manager's own F8), `N`/`Esc` cancels with nothing touched.

- [x] Confirmation prompt + actual delete, single entry under the
      cursor (`keymap.rs::ConfirmDeleteCommand`,
      `main.rs::handle_confirm_delete_key`)
- [x] Multi-select — `F8` deletes every entry currently marked in the
      active panel (`Panel::marked_or_current`, shared with F5/F6's own
      `transfer_sources`) if any are, falling back to the cursor entry
      otherwise. `PendingDelete` now holds `entries: Vec<DeleteEntry>`
      rather than a single path/name/is_dir/size; a failure on one entry
      is only logged and doesn't stop the rest. The popup shows the
      name for a single entry or `"N items"` + the shared parent
      directory for several, mirroring the transfer popup's own
      single-vs-multiple wording
- [ ] A failed delete (permissions, file in use, ...) is only logged
      (`debug!`), not shown to the user — no status-bar message surface
      exists yet (same gap as the non-UTF-8-file case above)
- [ ] No "move to Recycle Bin" option — always a hard, permanent delete
