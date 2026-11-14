# Fuzzing

Targets (all in `fuzz_targets/`, using `vault-core` only):

| Target | What it checks |
|---|---|
| `payload_decode` | Arbitrary decrypted-payload bytes either fail cleanly or decode into a vault that re-encodes and re-decodes identically. |
| `encrypted_import` | Arbitrary bytes as an age file never panic; header parsing and the scrypt work-factor bound are exercised; plaintext output stays within the requested bound. Inputs containing an `scrypt` stanza skip the (legitimately slow) KDF. |
| `revision_validation` | Byte-driven sequences of edits, saves (auto/explicit), restores, trash, folder and move operations always leave a vault that validates and round-trips. |
| `markdown_parse` | The inert preview parser never panics on any UTF-8 input. |
| `text_ops` | Editor operations (indent, outdent, list continuation, task toggle) always produce edits on char boundaries with in-range selections. |

Synthetic seed inputs live in `seeds/<target>/` and are committed; the
working corpus in `corpus/` is generated and ignored by Git.

Requirements: a nightly toolchain and cargo-fuzz.

```bash
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

Bounded smoke run (what CI does, ~30 s per target):

```bash
cd fuzz
for t in payload_decode encrypted_import revision_validation markdown_parse text_ops; do
  mkdir -p corpus/$t
  cargo +nightly fuzz run $t corpus/$t seeds/$t -- -max_total_time=30 -rss_limit_mb=4096
done
```

Longer local runs: raise `-max_total_time` (e.g. `3600`) and add
`-jobs=$(nproc)`. Any crash is written to `artifacts/<target>/`; minimize it
with `cargo +nightly fuzz tmin <target> <artifact>` and add the minimized,
synthetic reproducer as a regression test in `crates/vault-core/tests/`.

Recorded local smoke results (macOS arm64, rustc nightly, 2026-11-14):
payload_decode ≈1.27M runs/21 s, encrypted_import ≈0.93M/21 s,
revision_validation ≈0.25M/41 s, markdown_parse ≈1.10M/31 s, text_ops
≈1.74M/31 s — no crashes.
