# Git integration

Requested directly, three related but separable pieces — none scoped
in detail yet, don't start implementation from these bullets alone:

- [ ] **Core git mechanism** — how litastum actually talks to a git
      repo at all; needs deciding before either bullet below can be
      built on top of it. Options: shell out to the user's own `git`
      binary (simplest, matches this project's existing command-line
      shell-out pattern in `command_line.rs::run_command_line`, no new
      dependency, but output-parsing is inherently fragile against
      different git versions/locales), or a Rust library
      (`git2`/`libgit2` bindings — a C dependency, the same tradeoff
      `onig`/`syntect` already introduced, see
      `.claude/rules/litastum-stack.md`'s "Known risk" note — vs. pure-
      Rust `gix`, worth checking how complete its plumbing/porcelain
      coverage is for what's actually needed here before committing to
      it). Whichever is chosen, this is also the natural place to detect
      "is the current panel directory even inside a git repo" for
      showing/hiding any git-aware UI at all.
- [ ] **Dependency search by file path** — find what depends on (or is
      depended on by) a given file, scoped to the current repo. Scope
      still unclear: language-aware import/`use`-graph analysis (a much
      bigger undertaking, would need a parser per language) vs. a
      simpler text-based search for the file's own path/module name
      appearing elsewhere in tracked files (closer to what
      `find_file.rs`'s existing content-search machinery could be
      extended to do, see [find-file.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/find-file.md)). Needs a
      decision on which of these is actually wanted before scoping
      further.
- [ ] **Commit search** — find commits by message/author/date/path,
      presumably via `git log` (or the equivalent plumbing call once the
      core mechanism above is chosen) with a filtering popup similar in
      shape to `find_file.rs`'s own search UI. Needs deciding: search
      scope (current file's history vs. whole repo), how results are
      presented/navigated (jump to a diff view — see the conflict-
      resolver placeholder in [next-up.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/next-up.md)), and whether
      this needs its own history/persistence the way
      [history.md](https://github.com/libertadtangostudi0/litastum/blob/main/TODO/history.md)'s feature does.
