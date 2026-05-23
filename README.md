# History Auditor

Standalone Rust workspace for evidence-backed transcript auditing, report
generation, and memory retrieval.

## Workspace

- `history-auditor-core`
  Transcript ingestion, typed extraction, workflow verdicts, conservative local
  repo probes, SQLite persistence, report rendering, retrieval, and memory
  compilation.
- `history-auditor-mcp`
  MCP handler and tool schemas over the core service.

## Integration model

The core crate does not know how any specific product stores sessions. An
integrating product provides a `SessionSource` adapter that returns normalized
`SessionSnapshot` records for `audit_session(...)`.

That keeps product-specific session storage, provider metadata lookup, and
filesystem conventions outside the standalone workspace boundary.

## Local verification

The repository documents and CI use the same verification steps:

```bash
cargo fmt --all -- --check
python3 scripts/verify_history_auditor_contract.py
cargo test --release --workspace
./scripts/verify-history-auditor-regressions.sh
cargo build --workspace --release
```

The focused regression script keeps release-mode coverage aligned with the
history-auditor contract in `docs/verification.md`.

## CI

GitHub Actions runs:

- formatting checks in `.github/workflows/fmt.yml`
- contract, workspace test, focused release regression, and release build checks
  in `.github/workflows/ci.yml`

## License

This repository is available under the 0BSD license. See `LICENSE`.

Third-party components retain their own terms.
