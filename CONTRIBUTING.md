# Contributing

Thanks for helping. Nilavar handles secrets, so a few rules are stricter
than usual.

## Rules for handling data

- **Never use real secrets or real vaults** in tests, fixtures, issues,
  screenshots or demos. Use obviously synthetic data (`example.invalid`,
  `pk_test_EXAMPLE_0000`, `NVCANARY-…` canaries).
- Never put passphrases or secrets in command-line arguments, environment
  variables, shell history, comments or fixtures. Tests feed passphrases
  via stdin or in-process.
- Don't inspect your home directory, browser profiles, keychains, SSH keys
  or other personal data while developing.
- Debug builds store data in a separate `…-dev` directory; set
  `NILAVAR_DATA_DIR` to a scratch directory for experiments.
- Run development builds you don't fully trust in a **separate OS account
  or VM**. `.gitignore` excludes `*.nvault.age` files but **ignore rules
  are not an access-control boundary**.

## Code rules (enforced by `scripts/check-hygiene.sh` and CI)

- No `dbg!`; no printing from `vault-core`; no logging/tracing frameworks.
- Secret-bearing types (`SecretText`, `Passphrase`) must not implement
  `Display` or serde traits; their `Debug` is redacted. Model types are
  serialized only by `vault-core/src/format.rs`.
- Errors are `ErrorCode`s with fixed messages; never interpolate document
  text, titles, passphrases or sensitive paths into messages.
- `vault-core` is `#![forbid(unsafe_code)]`. New `unsafe` elsewhere needs a
  `// SAFETY:` argument and a hygiene-script allowance.
- No network-capable dependencies (`deny.toml` bans HTTP/TLS clients and
  async network runtimes); keep egui's `persistence` feature off.
- Prefer zeroizing buffers for plaintext; avoid needless clones.

## Development

```bash
cargo test --workspace                     # unit, integration, UI (macOS) and interop tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
./scripts/check-hygiene.sh
gitleaks git --redact --config .gitleaks.toml .
cargo deny check && cargo audit
```

- Interop tests drive the reference `age` and `rage` tools when installed
  (`brew install age`, `cargo install rage`); set
  `NILAVAR_REQUIRE_INTEROP=1` to make missing tools a failure.
- UI tests (`crates/vault-desktop/tests/ui_flows.rs`) run headlessly with
  egui_kittest + wgpu; set `NILAVAR_SCREENSHOTS=<dir>` to write PNGs.
- `NILAVAR_DEV_WORK_FACTOR=10` (debug builds only) makes manual testing
  faster by lowering the scrypt cost of *new* writes; never use it with
  real data.
- Fuzzing: see [fuzz/README.md](fuzz/README.md).

## Pull requests

- Keep changes focused; include tests for behavior changes, especially for
  anything touching storage, crypto, validation, locking or plaintext
  lifetimes.
- Update docs (FORMAT.md for any payload change — schema changes need a
  version bump and migration).
- CI runs on untrusted PRs with read-only permissions and no secrets.
