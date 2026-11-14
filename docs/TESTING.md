# Testing

```bash
cargo test --workspace            # everything below (UI flows need macOS/Metal or another wgpu backend)
NILAVAR_REQUIRE_INTEROP=1 cargo test -p vault-core --test interop   # fail if age/rage are missing
NILAVAR_SCREENSHOTS=docs/screenshots cargo test -p vault-desktop --test ui_flows
```

## Suites (108 tests at the time of writing)

| Suite | Count | What it covers |
|---|---|---|
| `vault-core` unit tests | 68 | Secret wrappers (redaction, no trimming, zeroizing edits), ids, model rules, history coalescing/retention/restore, payload encode/decode/validation, age adapter bounds, atomic writes and every fault point, symlink refusal, recovery-copy retention, session lock, import/export, lock state machine, autosave scheduler, clipboard policy, strength estimator, search/find/replace/fuzzy, text ops, Markdown model |
| `vault-core` doctests | 2 | `compile_fail`: secret types cannot be passed to the diagnostics API or formatted with `Display` |
| `tests/security.rs` | 16 | See the acceptance matrix below |
| `tests/interop.rs` | 4 | Committed fixtures from `age` 1.3.2 and `rage` 0.12.1 decrypt; our exports and whole vault files decrypt with both tools; files both tools write decrypt with vault-core (driven through a pty) |
| `vault-cli` `tests/cli.rs` | 7 | create/import/list/verify, wrong-passphrase exit code, encrypted export, plaintext export refusal + confirmation, change passphrase, recover, backup; no canary text on stdout/stderr |
| `vault-desktop` unit | 2 | Highlighter preserves text exactly; preview task-offset mapping |
| `vault-desktop` `tests/network_sandbox.rs` | 2 | macOS no-network sandbox denies TCP/UDP/listen but atomic saves work; screen-lock probe |
| `vault-desktop` `tests/ui_flows.rs` | 7 | Real app driven headlessly (egui_kittest + AccessKit + wgpu): wrong/right unlock, editor typing and ⌘S verified on disk, palette, history, ⇧⌘L lock wiping the workspace; unsaved-lock dialog save/discard/cancel; external modification → conflict copy; idle lock saves encrypted; welcome/create/empty/settings/narrow layouts; editor ⌘C routed through the clipboard guard and cleared on lock only if unchanged; preview links require confirmation |
| Fuzzing (`fuzz/`) | 5 targets | Payload decode, encrypted import, revision validation, Markdown, text ops (bounded smoke in CI) |

Plus repository checks: `scripts/check-hygiene.sh`, `gitleaks --redact`,
`cargo deny check`, `cargo audit`, `cargo clippy -D warnings`, `cargo fmt`.

## Security acceptance matrix

| Requirement | Test |
|---|---|
| Round-trip Unicode, empty and large-within-limit documents and complete vaults | `security::roundtrip_unicode_empty_and_large_documents`, `format::tests::roundtrip_preserves_everything` |
| Wrong passphrase, flipped bytes, reordered/replaced chunks, truncated header/body/end fail without output | `security::flipped_bytes_anywhere_fail_without_output`, `truncation_at_header_body_and_end_is_detected`, `reordered_and_replaced_chunks_are_detected`, `crypto::tests::wrong_passphrase` |
| age/rage interoperability both directions | `interop::*` (fixtures always; live tools when installed, required in CI) |
| Unknown schema, oversized data, excessive KDF, invalid folder graphs, duplicate ids fail within bounds | `security::malformed_payloads_rejected_and_file_untouched`, `oversized_inputs_rejected_before_work`, `excessive_work_factor_rejected_quickly` |
| Faults at write/flush/replace keep a complete vault | `security::injected_faults_keep_a_complete_vault`, `storage::tests::every_fault_preserves_previous_file_and_cleans_temp` |
| Oversized save refused without discarding edits or pruning history | `security::oversized_save_is_refused_without_losing_edits` |
| Passphrase-change failure keeps old credentials; success opens on a clean install | `security::passphrase_change_failure_keeps_old_credentials`, `cli::change_passphrase_then_old_fails_new_works` |
| External modification and two instances never overwrite each other | `security::external_modification_and_two_instances_never_overwrite`, `ui_flows::conflict_is_detected_and_conflict_copy_written` |
| History, trash, backup and restore recover exact contents | `security::history_trash_backup_and_restore_recover_exact_contents` |
| Lock during save, idle lock, save failure on lock, worker cancellation | `lifecycle::tests::*` (never "locked" while workers hold secrets), `security::auto_lock_policy_save_then_recovery_copy_then_discard`, `ui_flows::idle_lock_saves_dirty_session_encrypted`, `ui_flows::unsaved_manual_lock_offers_save_discard_cancel` |
| Preview cannot fetch remote content, execute HTML or include files | `markdown::tests::html_is_inert_literal_text`, `images_become_placeholders_and_links_are_data`, `ui_flows::preview_links_require_confirmation_and_never_open_directly` |
| Clipboard timeout does not erase newer unrelated data | `clipboard::tests::never_erases_newer_unrelated_content`, `ui_flows::editor_copy_goes_through_guard_…` |
| Secret types cannot enter diagnostics without explicit conversion | `compile_fail` doctests in `secret.rs`; `diagnostics::tests::report_contains_only_codes` |
| Canary leakage scan (logs, app data, temp/recovery files, exports, reports; raw/UTF-16/hex/base64) | `security::canaries_never_reach_disk_logs_or_diagnostics`, `recovery_copies_are_encrypted_and_bounded`, `cli::*` stdout/stderr checks |
| Secret cleanup at lock | `ui_flows::unlock_edit_save_lock_end_to_end` (session dropped, tabs cleared, no document text in the accessibility tree after lock). Limitation: tests cannot inspect freed process memory or OS caches. |
| No app-originated network activity | Dependency policy (no network crates); `network_sandbox` test (kernel denial on macOS); packaged app observed with `lsof -i` showing zero sockets during an idle run (manual, macOS, 2026-11-14) |

## Implemented / tested / deferred

| Area | Implemented | Automated tests | Manually verified | Deferred / limits |
|---|---|---|---|---|
| Vault format, age adapter, validation, limits | ✅ | ✅ unit + security + fuzz | — | — |
| Atomic saves, conflicts, recovery copies, backups | ✅ | ✅ | — | Race window with sync clients (documented) |
| History, trash, restore | ✅ | ✅ | — | — |
| Import / encrypted & plaintext export | ✅ | ✅ (core + CLI) | — | GUI file dialogs not automated |
| Recovery CLI | ✅ | ✅ | — | — |
| Lock lifecycle, idle/sleep/screen lock, auto-lock policy | ✅ | ✅ (idle via UI test, policy in core) | — | Sleep/screen-lock signals not automatable in CI; screen lock macOS-only |
| Editor (tabs, find/replace, list continuation, indent, line numbers, highlight) | ✅ | Partly (text ops, highlighting, typing + save via UI) | Rendering reviewed in screenshots | IME composition untested |
| Search, palette, quick switcher | ✅ | Partly (core search; palette UI) | — | — |
| Markdown preview | ✅ | ✅ | Screenshots | — |
| Clipboard guard and privacy hints | ✅ | ✅ (policy + UI with fake backend) | — | OS privacy-hint effect not verified |
| Diagnostics report | ✅ | ✅ | — | — |
| macOS no-network sandbox | ✅ | ✅ | ✅ `lsof` on packaged app | Linux/Windows: not implemented |
| Core-dump disabling | ✅ Unix | — | — | Windows: not implemented |
| macOS packaging (ad-hoc signed app, zip, SBOM, checksums) | ✅ | — | ✅ built and launched locally | Developer ID signing/notarization: documented, not performed |
| Linux / Windows packaging | ✅ scripts | CI only | ❌ not run on real desktops | — |
| HiDPI | egui scaling + interface-scale setting | — | Rendered at 1× in tests | Not visually verified at 2× in this environment |
| Accessibility | AccessKit labels on custom widgets | ✅ (UI tests query by label) | — | Screen-reader pass not done |
| Independent security audit | — | — | — | Not done (prerequisite for stronger claims) |

## Performance notes

`large_document_frame_times` (ignored by default; run with `--release
--ignored`) opens a ~1.9 MiB document in the headless harness: ~30 ms idle
and ~80 ms typing frames on an Apple M5 Pro. Profiling (`sample`) shows about
two thirds of that time in egui's AccessKit text updates, which the harness
always enables; in the real app AccessKit is built only while assistive
technology is connected, so very large documents are slower with a screen
reader active. Saving costs ~0.31 s (scrypt) on a worker thread.

## Interactive GUI note

The automated UI tests render the real application offscreen. In the
development environment used for this alpha the display could not be
captured with `screencapture`, so the screenshots in `docs/screenshots/`
come from the headless renderer (same code path, wgpu/Metal). The packaged
app was launched and observed (process health, CPU, sockets) but not
clicked through by hand.
