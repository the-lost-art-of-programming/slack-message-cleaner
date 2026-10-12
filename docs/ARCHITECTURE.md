# Architecture and acceptance criteria

**Target:** Firefox Manifest V3 WebExtension with Rust/WebAssembly + Leptos UI,
build-generated JavaScript interoperability only, browser-local persistence, and no backend server.

## Planned components

| Unit | Responsibility | Effect boundary |
| --- | --- | --- |
| Rust core | Message/identity types, pure selection and state transitions | None |
| Discovery orchestrator | Conversation/page cursors, deduplication, scan completeness | Slack client port |
| Deletion orchestrator | Immutable batch, revalidation, throttling and checkpoints | Slack/storage/clock ports |
| Firefox adapter | Session authorization, permissions and lifecycle | Browser APIs |
| Leptos UI | Filtered list, select, confirm, cancel, progress | Commands only |
| IndexedDB adapter | Scans, message metadata, batches and checkpoints | IndexedDB |
| Generated interoperability | Build-produced WASM loader and necessary Firefox event registration glue; no hand-maintained JavaScript source | Browser APIs |

## State machine

`Discovered -> Selected -> Confirmed -> Queued -> InFlight -> Succeeded`
with explicit `Skipped`, `Failed`, `UnknownOutcome`, and `Cancelled` states.
No transition from `Discovered` directly to deletion is legal. Resume loads
snapshots from IndexedDB and must reconcile uncertain outcomes before retrying.

## Scanning model

Enumerate conversations available to the user, paginate through accessible
history and thread replies, deduplicate by `(workspace, channel, ts)`, and
record scan scopes that are incomplete. Slack API endpoint access with a web
session is an *unverified implementation assumption*, not a guaranteed route
around administrator restrictions. Keep an alternate UI automation adapter
behind the same interface if permitted UI functionality exists.

## Permission policy

Use the narrowest host match patterns and Firefox extension permissions.
Prompt explicitly before accessing a Slack session; do not transmit or log
credentials. Avoid `webRequest` except if user explicitly enables controlled
session integration; keep session data in memory and bind it to workspace.

## Milestones and acceptance gates

1. Core types, selection and ownership invariants, property/mutation tests.
2. Read-only scan with fixture-based completeness/error telemetry.
3. Browser auth adapter, tested in an explicitly opted-in workspace.
4. Leptos message dashboard, pagination, select-all scope, persistence.
5. Dry-run deletion engine with deterministic fixtures and checkpoint resume.
6. Live opt-in bounded deletion test, Firefox packaging, audit/release checks.

The initial pure-Rust implementation is in `crates/cleaner-core`. No working Firefox add-on or Slack adapter exists yet. See `IMPLEMENTATION_PLAN.md` for sequence and `STAGE0_FINDINGS.md` for platform gates.
