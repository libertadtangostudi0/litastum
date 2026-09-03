# Logging

## Level semantics (observed convention, keep following it)

- `debug` — routine, expected state changes: UI interactions, spawns/
  despawns, track switches, asset-not-loaded-yet-this-frame. High
  volume is fine; this is what you turn to first when tracing "what
  actually happened."
- `info` — significant, one-off events: a move accepted, game over, an
  analysis thread starting/stopping. Notably less frequent than
  `debug`.
- `warn` — a recoverable problem worth noticing (e.g. "no audio loaded
  for this track") — something's off but execution continues sensibly.
- `error` — an actual failure (a caught panic, a decode error). Should
  be rare; if something logs `error` routinely, that's itself worth
  fixing rather than living with.