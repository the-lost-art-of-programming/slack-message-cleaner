# Verification report — 2026-10-11

## Scope

New Rust/Leptos implementation source now includes browser token capture,
read-only Slack conversation/thread discovery, review navigation, selection,
explicit count confirmation, durable queue and deletion-attempt preflight.
`live-delete` is OFF in the default build pending user-authorized testing.

## Checks actually performed

| Check | Observed result |
| --- | --- |
| Python structural/security suite (`scripts/static_check.py`) | PASS: 5/5 |
| `Cargo.toml` and `Trunk.toml` syntax | PASS via Python `tomllib` |
| Firefox manifest JSON syntax, sidebar, permissions | PASS via Python JSON checks |
| No handwritten `.js` / `.ts` files | PASS |
| Rust compiler / Cargo / Trunk presence | UNAVAILABLE |
| Rust compilation, rustfmt, Clippy, Rust unit tests | NOT RUN |
| WebAssembly generation and Firefox MV3 package | NOT BUILT |
| Headless Chromium HTML smoke attempt | TIMED OUT (18 seconds); no browser pass |
| Headless Firefox/geckodriver | NOT RUN: Firefox/geckodriver absent |
| Slack auth, discovery API, token capture, deletion | NOT RUN: no authenticated workspace |

A runnable browser extension **does not exist yet in this environment**. The
source archive is provided so a Rust-capable machine or CI job can build and
perform real headless Firefox installation/UI tests. The smoke harness in
`scripts/firefox_smoke.py` is not itself evidence of a successful test run.

## Critical acceptance blockers

1. Compile the host core and WASM UI and resolve any compiler/lint problems.
2. Verify generated Trunk JavaScript is allowed by Firefox extension CSP.
3. Headlessly install the actual package in Firefox, load Leptos, and verify
   the token-capture API is available.
4. Test `auth.test` and the conversation/replies endpoints with a signed-in
   member of a workspace that restricts installing apps.
5. Confirm per-message preflight is reliable for thread replies before
   considering a carefully controlled live deletion test.
6. Confirm rapid browser termination produces an interrupted checkpoint and
   no request resumes without another explicit user click.

No messages were accessed or deleted during this authoring session.
