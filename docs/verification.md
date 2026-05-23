# Verification

## History Auditor Contract

The native enforcement artifacts for this repository are:

- `.github/workflows/ci.yml`
- `.github/workflows/fmt.yml`
- `scripts/verify_history_auditor_contract.py`
- `scripts/verify-history-auditor-regressions.sh`

They enforce the extracted history-auditor contract by:

- checking that `docs/ARCHITECTURE.md` and this verification document still
  describe the standalone crate split, the `SessionSource` boundary, and the
  expected enforcement artifacts
- checking that the CI workflows still run formatting, contract verification,
  workspace tests, focused release regressions, and a release build
- checking that the focused history-auditor regression tests still exist in the
  expected source files under `crates/history-auditor-core/src`
- running the focused release regression bundle that covers session audit,
  artifact audit, retrieval shaping, memory promotion, and structured
  extraction behavior

## Local and CI commands

The documented local verification path and `.github/workflows/ci.yml` should
stay aligned:

- `cargo fmt --all -- --check`
- `python3 scripts/verify_history_auditor_contract.py`
- `cargo test --release --workspace`
- `./scripts/verify-history-auditor-regressions.sh`
- `cargo build --workspace --release`
