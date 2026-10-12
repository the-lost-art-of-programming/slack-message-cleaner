# Development conventions — Firefox Slack message cleaner

This document adapts the principles in
[praxis-proxy/conventions](https://github.com/praxis-proxy/conventions),
primarily its `docs/conventions.md`, root `Cargo.toml`, and tooling configs.
The upstream repository is Apache-2.0 licensed; our configuration and design
are adapted, not represented as an upstream-approved project.

## Guiding principles

1. **Brevity without ambiguity.** Small modules and functions with a single
   responsibility. Avoid clever abstractions that obscure data flow.
2. **Functional core, imperative shell.** Parsing, filtering, authorization,
   queue-state transitions, pagination planning, scheduling decisions, and
   deduplication are pure Rust operations. Side effects occur only through
   explicitly injected browser, Slack, storage, and clock adapters.
3. **Data ownership is explicit.** A message cannot become deletion-eligible
   merely by appearing in the search results. Author and workspace identity
   must be established and checked again immediately before deletion.
4. **Make invalid states unrepresentable.** Typed IDs, enums for lifecycle
   states and errors, validated timestamps, and distinct discovered/selected/
   confirmed/deleted states are preferred to flags and untyped maps.
5. **Determinism and testability.** Stable IDs and ordering, pure test fixtures,
   injection of time/randomness/network response, bounded retries and queues.
6. **Minimal permissions and data retention.** Access only the Slack domains
   needed, never collect unrelated browsing data, and avoid persisting tokens.
7. **Performance with evidence.** Avoid excessive cloning and browser DOM
   churn, but optimize only measured bottlenecks in long conversation scans.

## Rust coding style

- Rust 2024, a pinned stable toolchain (and nightly only for rustfmt options).
- All crate roots prohibit `unsafe_code`; use strict workspace lint settings.
- No panic/unwrap/expect in either production code or tests. Fallible tests
  return `Result` and propagate with `?`.
- No direct `as` casts or unchecked integer arithmetic. Handle overflow by
  intent and represent errors explicitly.
- Enumerate every variant when matching enums, especially deletion/job states.
- Use structs for known JSON keys, `#[serde(deny_unknown_fields)]` for owned
  configs, and tolerant decoding for Slack responses that intentionally expose
  forward-compatible unknown fields.
- Use BTreeMap/BTreeSet or sorted vectors where iteration order affects output.
- No `HashMap` iteration order dependencies. No raw string range indexing.
- Accept references and borrowed data where reasonable. Minimize clones.
- `thiserror` for typed errors, `tracing` for structured events. Logs may
  include redacted error categories or request IDs, never content or tokens.
- Imports and `#[derive]`/`#[serde]` modifiers ordered consistently.
- Use `foo.rs` plus `foo/` rather than `foo/mod.rs`.
- Prefix neither tests nor types with redundant words; names describe intent.
- Keep functions near the upstream's 30-line Clippy threshold; do not blindly
  split logically indivisible effect orchestration.
- Comments explain surprising constraints rather than line-by-line mechanics.
- Document all items with concise rustdoc of intent and caller contract. Prefer
  docs tests for public pure functions. No runtime-sequence narration in docs.
- Source order: constants; public API; private types; utilities; tests last.
  Separate major sections with the upstream full-width (77-hyphen) divider.

## Functional boundaries

Pure core modules (planned): `identity`, `message`, `selection`,
`discovery_plan`, `deletion_policy`, `queue_state`, `rate_limit`, and
`checkpoint`. A narrow `ports` layer defines effects. Adapters implement
Slack web requests, Firefox WebExtensions API, IndexedDB, timers, and the
Leptos UI. Only deterministic **generated** JavaScript glue connects Rust/WASM to Firefox-specific browser APIs; no handwritten JavaScript/TypeScript application source is allowed by default.

No core function reads global browser state, sends HTTP, or accesses the DOM.
Construct immutable command and event values. Reducers return new state and
follow-up commands, leaving execution to the effects adapter. Prefer a bounded
worker rather than unbounded concurrent deletions.

## Deletion safety contract

- Discovery is read-only and must explicitly report incomplete coverage
  (permissions, private conversations, retention, search windows, errors).
- Display the message, conversation, timestamp, and deletion eligibility.
- Select messages by stable ID only; "select all" affects a defined result
  set, never hidden/unloaded messages without an explicit scope confirmation.
- Before a destructive job begins, show exact count and workspace and obtain
  explicit confirmation. The queue snapshot is immutable after confirmation.
- Revalidate Slack identity and message ownership for each deletion attempt.
- Successful deletion is recorded only after a verified success response.
  Already deleted / inaccessible / ambiguous outcomes are separate statuses.
- Page scans, deletions, and retries obey measured rate limits with jittered
  bounded backoff and cancellation. Cancellation stops future requests and
  leaves an auditable local checkpoint.
- The extension must not claim to operate while Firefox is closed.
- Session-token capture is an optional, locally constrained transport method,
  not a promise to bypass disabled APIs, policy, or workspace permissions.

## Testing policy

- Unit tests for every pure function and every regression.
- Property-based tests (`proptest`) for message-key uniqueness, order stability,
  idempotent queue transitions, ownership restrictions, and pagination cursors.
- Mutation tests for security-relevant branches, not just line execution.
- Cargo LLVM coverage target: >=90% line and >=80% region for host core. WASM
  and browser adapter quality is evidenced with separate Firefox integration
  tests and cannot be inferred from host coverage percentages alone.
- Recordable mocked Slack responses must include empty pages, mixed authors,
  threads, duplicates, deleted messages, expired tokens, HTTP 429 responses,
  transient failures, partial scans, and permission-denied cases.
- Integration tests isolate browser effects from the core; real destructive
  tests require a disposable user-owned test workspace and explicit opt-in.
- Test functions have descriptive names without `test_`, include contextual
  assertion messages, and avoid inline explanatory comments.

## Tooling and review requirements

- `cargo fmt` (nightly options in `rustfmt.toml`), `cargo clippy` with
  `-D warnings`, `cargo test`, `cargo llvm-cov`, `cargo mutants`, `cargo audit`,
  `cargo deny check`, `cargo machete`, Taplo, typos, markdownlint, and actionlint.
- Check host and `wasm32-unknown-unknown` builds, pinned compiler minimum,
  browser manifest validity, and Firefox end-to-end test fixtures.
- GitHub actions should be least-privilege and pinned to commit SHAs.
- Conventional commits; deliberate PR scope; explain changes, risks, and test
  evidence. Use human review rather than asserting an AI sign-off.
- Documentation prose targets 80 columns, Rust source 120 columns; mark code
  fence languages and keep tooling settings in sync.

## Deliberate deviations from the upstream template

- `tokio`, Kubernetes, Docker/Podman, server-specific banned methods,
  interactive stdin prohibitions, server containers and crates.io publishing
  are not prerequisites for a browser extension.
- WebAssembly/browser promises can be `!Send`; adapt `future_not_send` only in
  a tightly scoped browser adapter, never by weakening host-core guarantees.
- Externally controlled Slack JSON is forward-compatible and cannot always
  apply `deny_unknown_fields`, unlike our own strict configuration files.
- Pure core tests use host coverage/mutation thresholds; browser APIs also need
  Firefox/real-DOM tests because LLVM host coverage alone misses them.
- Releases package unsigned/signed browser extension artifacts and optionally
  Firefox AMO submissions rather than server containers or crates.io artifacts.
- No remote automated-review service is required: do not send Slack data or
  source secrets to third-party review services as part of CI.
