# Test scratch directories (`test_support.rs`) -- history

A batch of inexplicable failures -- `AlreadyExists` creating a directory
that was supposedly just made, listings finding a file the test never
wrote -- came from scratch directory names colliding with an earlier
run's. Names were PID + a counter restarting at 0; Windows reuses PIDs,
and nothing ever deleted old scratch directories (over 69,000 had piled
up under the temp dir).

- A nanosecond timestamp in the name fixed the collision itself.
- `cleanup_stale_scratch_dirs` sweeps the app's own leftovers once per
  test binary (requested instead of a one-off manual deletion), behind a
  `Once` that also blocks other threads until it's done, so a fresh
  directory can't be deleted mid-sweep.
