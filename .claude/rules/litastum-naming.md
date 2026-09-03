# litastum: naming and licensing

## Name

`litastum` — checked for collisions against several rejected candidates:
- `medusa` collides with a well-known pentest brute-force tool
- `janus` collides with Meetecho's WebRTC gateway
- `selene` collides with a popular Rust-written Lua linter
- `nox` collides with NoxPlayer (Android emulator) and the Python `nox`
  test-automation tool

## License

`MIT OR Apache-2.0` (Rust ecosystem convention, already set in
`Cargo.toml`). `LICENSE-MIT` is present. `LICENSE-APACHE` is deliberately
not added yet — add it if `Apache-2.0` actually becomes needed (e.g. a
dependent project requires it), not preemptively.
