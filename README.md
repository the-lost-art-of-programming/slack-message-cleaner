# Own Message Cleaner — Rust / Leptos Firefox extension

**Status: uncompiled alpha source; live deletion disabled by default.** All authored application and UI code is
Rust; Trunk will generate the JavaScript glue needed to load WebAssembly.

## Intended workflow

1. Open Slack in Firefox. Open the extension sidebar and reload Slack to
   capture your existing session token temporarily (no Slack app install).
2. Connect to verify your Slack user/workspace with `auth.test`.
3. Scan accessible channels, private channels, DMs, group DMs and replies.
   Missing permissions or failures produce a partial-coverage warning.
4. Navigate by conversation type, specific conversation or thread replies;
   search the loaded preview text and individually select messages.
5. Enter the exact number selected to confirm a fixed deletion batch.
6. In a deliberately unlocked test build, manually start/resume. For each
   message, verify identity and authorship
   against Slack before calling `chat.delete`. Confirmed outcome checkpoints
   are persisted before the next attempt.
7. Stop at any time. Closing Firefox never automatically resumes the job.

The session token is captured in Firefox `storage.session` (not on disk). The
review index and paused jobs persist in `storage.local`. A standalone tab can
host a run while the sidebar is closed, but Firefox must remain open.

## Build and verification

See [Build and test instructions](docs/BUILD_AND_TEST.md). This environment
cannot compile the Rust source or load a real Firefox extension; do not treat
static checks or Chromium smoke tests as evidence of a working Slack deletion
path. Live deletion must first be validated on a disposable message in a
workspace where you have permission to delete that message.

## Structure

- `crates/cleaner-core`: validated IDs, selection, filtering, paused queues.
- `crates/cleaner-ui`: Rust Leptos UI and thin browser/Slack adapters.
- `extension`: Firefox manifest and Trunk HTML/CSS inputs.
- `scripts/build.py`: full build and package entry point.
- `scripts/static_check.py`: offline structural safety checks.
- `AGENTS.md` / `docs/CONVENTIONS.md`: project development conventions.

No vendor backend, app installation, hidden telemetry, or credential export.
