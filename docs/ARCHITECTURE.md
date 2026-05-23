# Architecture

## Workspace root

The workspace root owns shared Cargo configuration, hook installation, the Rust
toolchain file, and repository-level verification scripts.

It also owns the release-facing repository metadata and CI workflows that keep
the extracted workspace publishable as a standalone project.

## Crate boundaries

### `history-auditor-core`

This crate owns the full auditor domain model and storage contract:

- normalized transcript ingestion for saved sessions, markdown chat, JSONL chat,
  shell logs, and imported provider-session JSON
- typed extraction for intent, actions, errors, claims, workflow verdicts, and
  incident patterns
- conservative local-only repo probing
- SQLite-backed audit persistence, report emission, and memory storage
- retrieval over reports and approved memories through `semble-rs`
- repeated-incident clustering and memory promotion through `semhash-rs`
- the generic `SessionSource` / `InspectedSession` / `SessionSnapshot` boundary

Product-specific session loading must stay out of this crate.

### `history-auditor-mcp`

This crate owns the MCP-facing interface:

- tool input schemas
- MCP handler wiring
- blocking-to-async bridge for the core service

It depends on `history-auditor-core` and should not reimplement extraction,
storage, or repo logic.

## Session boundary

`history-auditor-core` defines a generic session adapter contract:

- `SessionSnapshot` is the normalized transcript payload used for ingestion
- `InspectedSession` adds provider/session metadata and the logical source path
- `SessionSource` resolves a session id into an `InspectedSession`

This lets sibling products keep their own session-store code while reusing the
same auditor core.

## Sibling-app integration

The extracted repository is consumed through:

- a thin product adapter that implements `SessionSource`
- an MCP binary that constructs `HistoryAuditorService` with that adapter

Product-specific session-store code stays outside this workspace.

## Storage compatibility

Auditor state is written under a home-scoped application directory.

The standalone repository prefers a dedicated history-auditor root and retains
compatibility fallbacks for older product-specific storage locations that may
already exist on disk.

## Verification

The repository-managed enforcement surface is:

- `.github/workflows/ci.yml`
- `.github/workflows/fmt.yml`
- `scripts/verify_history_auditor_contract.py`
- `scripts/verify-history-auditor-regressions.sh`

The contract script checks that the architecture and verification documents keep
the required ownership and testing anchors, the CI workflows keep the expected
verification commands wired, and the regression script runs the focused
release-mode auditor test bundle.
