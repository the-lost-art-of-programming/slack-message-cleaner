# Rust code layout and documentation

This project adapts [Praxis conventions](https://github.com/praxis-proxy/conventions)
for a Rust + Leptos Firefox WebExtension. Read with `AGENTS.md` and
`docs/CONVENTIONS.md`. These rules apply to all new modules and refactors.

## Exact Praxis section separators

Use the three-line separator, with 77 hyphens, around coherent groups:

```rust
// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------
```

Add one blank line on either side. Useful groups include Constants,
Domain types, Validation helpers, Browser boundaries and Tests. Keep
short modules without unnecessary decorative separators.

## Functional composition

- Prefer small pure functions, typed immutable records, and deterministic
  BTreeMap/BTreeSet iteration at core boundaries.
- Group validation, transformation, storage and external effects into distinct
  logical stages. Isolate browser timers, HTTP, credential capture and deletion.
- State invariants and externally verified facts directly, without guessing
  Slack permissions or assuming a scan is complete.
- Never use message preview text, DOM array indexes, or floating point
  timestamps as durable message identity.

## Purposeful code comments

Be generous with **why** comments: explain security invariants, session
trust boundaries, paginated scanning, interrupted operations, rate limits,
and why potentially ambiguous actions are not retried. Do not restate
obvious source syntax. Keep comments aligned with actual behavior.
Formatting and space should make control flow clear without visual noise.

## Rustdoc requirements

Every module starts with a `//!` description of its role and side-effect
boundary. Public types state important invariants and validity conditions;
public functions document observable contracts and failures. Public methods
returning `Result` need a precise `# Errors` section.

Prefer executable `# Examples` for pure APIs. Use `?` with a
result-returning example so tests need no `unwrap()`/`expect()`.
Examples should be credential-free, deterministic, and executable by:

```sh
cargo test -p cleaner-core --doc
RUSTDOCFLAGS='-D warnings' cargo doc -p cleaner-core --no-deps
```

Do not add misleading examples to effectful APIs that require Firefox,
an authenticated Slack workspace, or actual network access. Verify those
with mocked integration tests and a real headless Firefox smoke test.

## Code review and CI

Require strict Clippy (`-D warnings`), nightly rustfmt, unit/integration
tests, executable Rustdoc, supply-chain policy, offline safety tests, and
Firefox/WASM packaging validation. Never weaken lints merely to turn CI
green. Address failed tests or document narrowly scoped external exceptions.

Gloo dependencies belong to the Rust/WebAssembly ecosystem; they are
unrelated to Solo.io and are retained subject to normal dependency review.
