# Implementation plan — Rust/Leptos Firefox Slack message cleaner

**Status:** Rust/Leptos implementation source and Firefox manifest have been
authored, but compiler execution, packaged Firefox testing, and Slack integration
remain unverified. Live deletion is disabled in default builds. See `STAGE0_FINDINGS.md` for research and blocked gates.
This plan must not be interpreted as verified Slack access or tested browser behavior.

## 1. Product goal and definition of success

A Firefox extension that, with the user's explicit consent, uses their existing authenticated
Slack browser session to **find messages authored by their own account**, present a searchable,
reviewable, selectable index, and then process explicitly approved deletion batches safely.
It must not require installing a Slack app or provisioning OAuth access if the workspace's
existing user session already permits the needed operations. It must not claim to circumvent
workspace permissions or policy. If available access is insufficient, show a clear diagnosis
instead of silently performing an incomplete scan or unsafe fallback.

**North-star workflow:** Authenticate → discover (read-only) → review/filter → select →
confirm count and scope → queue → start/pause → monitor → reconcile/report.

## 2. Non-negotiable constraints

- **Source language:** Rust 2024 for all hand-authored application logic, policies, UI,
  orchestration, API adapters, and build-time generator logic. Leptos for dashboard rendering.
- **Interop reality:** Firefox needs JavaScript entrypoints, and Rust WebAssembly relies on
  generated JS bindings to interact with Web APIs. Allow machine-generated JavaScript in the
  output bundle, but no hand-authored `.js`, `.ts`, `.jsx`, `.tsx`, `.mjs`, or `.cjs` sources.
  Prove that Firefox MV3 background event listeners can be registered correctly before
  committing to a packaging design. If no robust solution exists under this constraint,
  ask the user rather than secretly introducing handwritten JavaScript.
- **Deployment:** Firefox-first, extension-local only, no external server, no telemetry/CDNs.
- **Compatibility:** Manifest V3; Firefox currently uses extension background pages/scripts,
  not an MV3 extension background service worker. Short-lived event page lifecycle is a
  major design constraint; never trust in-memory job state alone.
- **Authorization:** Work only with the actual logged-in user's permissions; no escalation.
  Tokens are secrets. Prefer ephemeral capture, scoped to the identity/workspace; do not
  log or export them. Decide any persisted-credential mode separately and explicitly.
- **Safety:** Read-only discovery; exact author/workspace validation; explicit item
  selection; immutable deletion-job snapshot; confirmation of exact quantity;
  controlled rate; clear pause/cancel; status reconciliation for unknown outcomes.
- **Conventions:** Adopt `AGENTS.md`, `docs/CONVENTIONS.md`, Rust lint/toolchain config,
  pure-core/effect-shell separation, comprehensive tests, mutation/property testing.

## 3. Proposed architecture

```text
Firefox user session (existing Slack tab)
                 |
        Browser boundary (Rust bindings / generated interop)
                 |
      Auth + capability probe (read-only, verified identity)
                 |
      Scan adapters: search | conversations/history | replies
                 |
       Rust domain core (pure functions, typed identities)
                 |
   Local index (IndexedDB) <----> Leptos dashboard (WASM)
                 |
            Explicit selection
                 |
          Confirmed immutable job
                 |
        Durable job executor + checkpoint store
                 |
          Slack deletion endpoint / UI fallback*

* UI fallback requires a separate validated feasibility decision.
```

### Crate boundaries (tentative)

| Crate | Scope | Test surface |
| --- | --- | --- |
| `cleaner-core` | Types, ownership/selection rules, stable IDs, dedupe, state transitions, retry classifications | Pure native unit/property/mutation tests |
| `cleaner-slack` | Endpoint semantics, response decoding, paging, rate limits, auth capability probe | Mocked request/response fixtures |
| `cleaner-store` | Repository interface, durable scan/job checkpoints, schema migration | Host fake + Firefox IndexedDB tests |
| `cleaner-firefox` | Session integration, WebExtension lifecycle and permission wrappers | Firefox integration fixtures |
| `cleaner-ui` | Leptos dashboard, virtualization, filters, exact selection scope, diagnostics | Component and real Firefox tests |

Keep each crate only if the boundary saves complexity. Start with `core` and a thin adapter;
do not create unused abstractions up front. Use native Rust tests wherever possible.

## 4. Discovery and data model

- First verify the Slack account and workspace ID; never rely on a display name alone.
- Probe available read-only capabilities individually. Not all browser session tokens will
  authorize Slack's documented endpoints or return the same data as interactive Slack.
- Preferred discovery: Slack search for the verified sender, then exact-identity filter.
  Alternative: available conversations → paginated history → thread replies, with scoped
  checkpoints and deduplication. A future UI-only discovery adapter is **not guaranteed**.
- Stable key: `(workspace_id, conversation_id, message_ts)`. Represent discovery source,
  verified author, thread parent, timestamp, human-readable preview, and coverage limits.
- Keep separate scan status per workspace, conversation, cursor, date window, and endpoint.
  Show incomplete access, private/DM exclusions, retention/search limits, inaccessible
  channels, failures, and truncation visibly. Never label a partial scan "all messages."
- Avoid saving more message content than necessary; support clearing the local index.

## 5. Deletion-job state machine

```text
Discovered -> Selected -> Confirmed -> Queued -> InFlight -> Succeeded
                                    |          |       +--> UnknownOutcome
                                    |          +----------> Failed / Skipped
                                    +---------------------> Cancelled
```

- Queue snapshots are immutable lists of stable message keys scoped to one identity/workspace.
- Each deletion precondition re-checks current user/workspace and message ownership when a
  reliable source is available; if it cannot be verified, skip or halt rather than guess.
- Process bounded batches, obey response rate limits and Retry-After, and cap concurrency.
- Persist checkpoint before and after each effect. A browser exit or event-page suspension
  cannot turn an uncertain deletion into a blind retry. Reconcile when possible.
- Deleting parent messages before discovering child replies is prohibited.
- Pausing must stop new work promptly. Cancellation must leave an auditable local summary.
- Browser closed means no background execution; dashboard closed while Firefox remains
  open may support queued execution if background lifecycle handling passes validation.

## 6. Staged delivery and acceptance gates

### Stage 0 — Feasibility and security spike (must pass before large build)

1. Validate Firefox MV3 package with Rust/WASM, Leptos and **generated-only** JS glue.
   Confirm permissions, CSP, restart behavior, and event-listener registration.
2. Test whether a signed-in Slack web session can be detected without unnecessary
   privileges and whether `auth.test`, search, history/replies, and `chat.delete` work
   in a consented test workspace. Never assume capture implies authorization.
3. Evaluate availability of a UI automation fallback without promising stability.
4. Document effective scopes and any workspace-level limitations; one-message deletion
   exercise only in an opt-in test workspace with disposable messages.

**Gate:** Reliable package load + account verification + at least one viable read path +
one controlled deletion path, or a documented stop/re-design decision.

### Stage 1 — Pure domain model

Define validated types, exact ownership invariants, scan completeness representation,
selection semantics, immutable job snapshots, retry policy, and state transitions.
Write property/mutation tests for deduplication, isolation, selection, and idempotence.

**Gate:** Native unit and property tests pass; forbidden transitions are rejected.

### Stage 2 — Read-only discovery

Implement capability probes, cursor checkpointing, search adapter, conversation crawler
and thread handling if available, de-duplication, progress and partial-coverage warnings.
Use fixture tests for pagination, restricted methods, rate limits, and malformed data.

**Gate:** A verified user can import their own permitted messages without any deletion.

### Stage 3 — Persistent Leptos review dashboard

Implement workspace/account badge, searchable/virtualized results, filters, message links,
select all *filtered* vs all *scanned* distinction, per-message select/deselect,
selection counts, and local index management. Store only necessary data in IndexedDB.

**Gate:** Large fixture lists remain usable; selections survive dashboard close/reopen.

### Stage 4 — Dry-run queue executor

Implement approval/count confirmation, immutable job creation, configurable batch sizes,
pace/rate-limit behavior, pause/cancel, deterministic logs without credentials,
crash/restart checkpoint recovery, and explicit unknown-outcome reconciliation.

**Gate:** No network deletion is possible in dry-run; simulated suspend/resume does
not duplicate effects, cross workspaces, or delete unselected messages.

### Stage 5 — Opt-in live deletion + Firefox release

Begin with one disposable message; expand to small batches only after verified outcomes.
Test rate limits, invalid session, changed workspace, deleted/edited messages, loss of
permission, and incomplete history. Finalize reproducible build, add-on signing plan,
local cleanup, permissions justification, dependency audits, and documented limitations.

**Gate:** All Rust checks plus WASM build, packaged Firefox integration tests and
manual live test checklist pass; no claim of universal Slack compatibility.

## 7. Verification strategy

- Rust: nightly rustfmt, Clippy deny warnings, `cargo test`, docs, `cargo audit`, `cargo deny`.
- Native pure core: >=90% line / >=80% region coverage targets; mutation tests focus on
  ownership, queue transitions, retry handling, and deduplication.
- Browser: real Firefox loading a packaged extension, IndexedDB, background suspension
  and restart, generated-script CSP compatibility, precise extension permissions.
- Slack: recorded redacted fixtures by default; separate user-authorized integration
  workspace with disposable messages for actual deletion; no CI job touches real messages.
- Release: no credentials in bundles, reports, snapshots, or logs; no external services.

## 8. Approved product decisions (2026-10-11)

1. **Language:** All handwritten application/UI code is Rust. Deterministic,
   build-generated browser JavaScript is permitted, but must satisfy the
   synchronous Firefox event-listener gate.
2. **Coverage and navigation:** Attempt all accessible public/private channels,
   one-to-one/group DMs and threads, with explicit navigational buckets and
   completeness reporting.
3. **Resumption:** Paused by default; the user must manually press Resume after
   browser restart or interruption. Unknown outcomes cannot be retried blindly.
4. **Credentials:** Recapture after a browser restart. Browser-session-only
   `storage.session` is a candidate for surviving Firefox event-page unloading;
   compatibility and permission checks are pending.

**Progress:** `cleaner-core` implements typed identities, selection, filters,
and a recoverable queue; `cleaner-ui` adds source for Leptos navigation, token
capture, Slack HTTP, read-only discovery, explicit confirmation and an opt-in
deletion runner. The source is **not compiled or browser-verified** in this
environment (Rust and Firefox are unavailable). The default build excludes
live deletion using a compile-time feature gate.

## 9. Non-goals and unresolved risks

- No attempt to override admin deletion policy, retrieve inaccessible/retained content,
  evade Slack security mechanisms, or delete other users' messages.
- No guarantee of discovering every historical message or confirming complete erasure from
  enterprise exports/retention systems.
- Browser session token access and Slack endpoint compatibility are *unverified*;
  UI-only automation may be brittle and may not achieve a full-workspace scan.
- Firefox MV3 background initialization from generated Rust/WASM interop has not yet been
  demonstrated and is the first stop/go engineering risk.

## 10. Reference documentation

- Firefox MV3 background: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background
- Leptos WASM interop: https://book.leptos.dev/web_sys.html
- Leptos CSR and generated JS loader: https://book.leptos.dev/getting_started/index.html
- Prior Slack proof of concept: https://github.com/sobakaio/slack-bulk-delete-extension
- Project coding philosophy: https://github.com/praxis-proxy/conventions
