# Stage 0 — Firefox, Rust/WASM, and Slack feasibility findings

**Date:** 2026-10-11
**Status:** Research completed; executable Firefox/Slack compatibility **not verified**.
**Implementation state:** A pure Rust core exists; it sends no network requests
and cannot delete messages. No Firefox extension package has been built.

## Confirmed platform behavior

| Topic | Finding | Engineering decision |
| --- | --- | --- |
| Firefox MV3 background | Firefox runs background scripts/event pages rather than MV3 background service workers. | Target `background.scripts` rather than Chrome's `service_worker` alone. |
| Event-page lifecycle | Persistent event listeners must be registered synchronously at top level. | Treat a generated WASM/JS bootstrap's synchronous registration as a **blocking compatibility test**. |
| Extension WebAssembly | MV3 needs `script-src 'self' 'wasm-unsafe-eval'` for extension pages. | Use packaged WASM only; prohibit remotely hosted code. |
| Sidebar | Firefox uses the `sidebar_action` manifest key and `browser.sidebarAction`, not Chrome `sidePanel`. | Implement the Leptos review UI as a Firefox sidebar. |
| Session credential storage | `browser.storage.session` is memory-only for the browser session and erased on shutdown. | Consider opt-in, session-scoped token retention; never store credentials in IndexedDB or `storage.local`. |
| Slack token capture pattern | The reference extension observes Slack API POST request bodies using `webRequest.onBeforeRequest` + `requestBody`. | Investigate a narrow read-only observer with explicit user consent and no third-party transmission. |
| Slack deletion | `chat.delete` only permits deletion allowed to the authenticated caller. | Keep revalidation and actual permission probes mandatory; do not claim admin restrictions are bypassed. |

These are documented platform/API behaviors, **not** proof that the proposed
Firefox add-on loads or can operate on a particular workspace.

## The central bootstrapping issue

Ordinary Rust-to-WebAssembly tooling creates a generated JavaScript loader,
but WASM initialization is usually asynchronous. Firefox extension background
listeners that are registered after asynchronous initialization may not be
persisted or woken correctly. A working UI bundle alone cannot clear this gate.

Viable approaches to evaluate **without handwritten JavaScript/TypeScript**:

1. Inspect Rust-first extension builders (including `oxichrome`) for a
   deterministic, generator-produced synchronous listener registrar; review
   output quality, licenses, dependencies, and Firefox behavior before adoption.
2. Keep all browser event callbacks in a generator-produced minimal bootstrap,
   with synchronous browser listener registration and only deferred dispatch to
   Rust/WASM. This is acceptable only if the generated artifact truly comes
   from audited Rust build tooling rather than an untracked handwritten JS
   template embedded in the repository.
3. Evaluate whether a sidebar-only extension is enough for the first read-only
   scanner, while treating close-sidebar continuation as a separate gate.

If robust background initialization cannot be demonstrated without handwritten
JS, **stop and obtain a user decision** before relaxing the source-language rule.

## Slack authorization limitations

The Chrome reference extension extracts an `xox...` token from an existing
Slack web API request body, then invokes Slack API methods using that captured
session. That demonstrates an implementation strategy, **not** a supported
universal API privilege or guarantee it will function for every workspace.

The extension should obtain explicit consent, capture only requests to Slack,
associate the token with an independently verified account/workspace, validate
token/capability errors, and offer disconnect/clear-session controls. It must
not treat a successful `auth.test` as proof that `conversations.list`,
`conversations.history`, `conversations.replies`, search, or `chat.delete`
are permitted. Each method needs separate read-only capability probes.

**Token handling:** A token is a bearer credential. Prefer `storage.session`
over persisted credentials to survive MV3 background unloads without surviving
browser restart. Browser version support, access-control defaults, and add-on
permissions must be tested in actual Firefox before enabling capture.

**Coverage:** Account identity, paginated accessible conversations, DMs,
threads, retention/search limits and permission failures all need explicit
coverage-status indicators. Never call partial discovery a full export.

## Zero-destructive first-pass validation steps

1. Install the pinned Rust toolchain and `wasm32-unknown-unknown` target; run
   core `cargo test`, Clippy, rustfmt, rustdoc, and supply-chain checks.
2. Build a minimal Leptos CSR sidebar and generated JS glue as a Firefox MV3
   add-on with CSP allowing only `'self'` and `'wasm-unsafe-eval'`.
3. Load the unpacked package in a disposable Firefox profile, verify event
   listener persistence after idle/background unload and browser restart.
4. Verify host permissions can be granted, reviewed, and revoked.
5. With explicit permission and a disposable Slack test workspace, verify
   ephemeral capture of the signed-in user's token and `auth.test` identity.
6. Probe read-only conversation, search, history and replies endpoints;
   record which methods fail or return partial data.
7. Check whether the target workspace permits deleting one's own messages
   **without deleting anything** initially. Only after separate user opt-in,
   test `chat.delete` on one deliberately created disposable message.
8. Document observed Firefox version, add-on permissions, endpoint responses,
   read coverage, and any blocked platform requirements before broad scanning.

## Current verification blockers

The execution environment in which this initial project was drafted lacks
`rustc`, `cargo`, `rustup`, `firefox`, a runnable test Slack workspace, and
network access from its container. The checked-in Rust source is **not yet
compiled or unit-tested**, and no running extension, token capture, or Slack
API request has been demonstrated. Static checks do not substitute for them.

## Primary references

- Firefox MV3 background: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background
- Firefox listener registration: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Background_scripts
- Firefox WebAssembly CSP: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/content_security_policy
- Firefox session storage: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/storage/session
- Firefox sidebar: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/sidebar_action
- Firefox network observer: https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onBeforeRequest
- Slack deletion semantics: https://docs.slack.dev/reference/methods/chat.delete/
- Reference browser token capture: https://github.com/sobakaio/slack-bulk-delete-extension/blob/main/background.js
- Rust-only extension framework candidate: https://oxichrome.dev/
