#!/usr/bin/env bash
# Repository checks for secret-handling rules (run in CI and before commits).
# These are tripwires; reviewers must still inspect the underlying code.
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0
bad() { echo "FAIL: $*"; fail=1; }

# 1. No dbg! anywhere in first-party code.
if grep -rn --include='*.rs' 'dbg!(' crates fuzz/fuzz_targets; then bad "dbg! is forbidden"; fi

# 2. No printing from the core library (examples are allowed).
if grep -rnE --include='*.rs' '\b(e?println!|e?print!)\(' crates/vault-core/src; then
  bad "printing from vault-core/src is forbidden (use the diagnostics API)"
fi

# 3. No logging/tracing frameworks anywhere (they could capture secrets).
if grep -rnE --include='*.rs' '\b(log::|tracing::|env_logger|#\[instrument)' crates; then
  bad "logging/tracing frameworks are not allowed in first-party code"
fi
if grep -nE '^(log|tracing|env_logger|tracing-subscriber) *=' crates/*/Cargo.toml; then
  bad "logging/tracing dependencies are not allowed"
fi

# 4. Model types must not derive serde traits (only format.rs serializes).
if grep -nE 'derive\([^)]*(Serialize|Deserialize)' crates/vault-core/src/model.rs crates/vault-core/src/secret.rs; then
  bad "model/secret types must not derive serde traits"
fi

# 5. Secret types must not implement Display.
if grep -nE 'impl (std::)?fmt::Display for (SecretText|Passphrase)' -r crates; then
  bad "secret types must not implement Display"
fi

# 6. egui persistence must stay disabled (it would serialize widget state).
if cargo tree -p vault-desktop -e features -i egui 2>/dev/null | grep -q '"persistence"'; then
  bad "egui/eframe 'persistence' feature is enabled"
fi

# 7. No HTTP/TLS/async-network client crates in the shipped dependency tree.
if cargo tree -p vault-desktop -p vault-cli -e normal --prefix none 2>/dev/null \
   | grep -Eiq '^(reqwest|hyper|ureq|curl|isahc|attohttpc|native-tls|rustls|tokio|async-std|surf) '; then
  bad "a network client crate entered the dependency tree"
fi

# 8. unsafe is only allowed in the documented platform functions.
unsafe_hits=$(grep -rn --include='*.rs' 'unsafe {' crates | grep -v 'crates/vault-desktop/src/platform.rs' | grep -v 'crates/vault-cli/src/main.rs' || true)
if [ -n "$unsafe_hits" ]; then echo "$unsafe_hits"; bad "unexpected unsafe block"; fi

# 9. Local vault artifacts must never be committed.
if git ls-files | grep -E '\.nvault\.age$' | grep -v '^crates/vault-core/tests/fixtures/'; then
  bad "vault files are tracked by git"
fi

if [ "$fail" -eq 0 ]; then echo "hygiene checks passed"; fi
exit "$fail"
