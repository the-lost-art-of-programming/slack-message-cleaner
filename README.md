# Own Message Cleaner — Rust / Leptos Firefox extension

**Status: early alpha; Firefox CI builds and smoke tests the WebAssembly UI; live Slack behavior remains unverified and deletion is disabled by default.** All authored application and UI code is
Rust; Trunk will generate the JavaScript glue needed to load WebAssembly.

## Intended workflow

1. Open Slack in Firefox, open the extension sidebar and refresh Slack once
   to capture your existing web-session token. No administrator or Slack app
   installation is required if your session can access the relevant endpoints.
2. Connect. The extension verifies your current Slack user and workspace.
3. **The extension actively fetches** all accessible channel/DM histories and
   every discovered thread using Slack pagination. You never have to scroll
   through Slack to populate its index. Partial results carry visible coverage
   warnings rather than pretending to be complete.
4. Browse **workspace → public/private channel or DM → thread → message**.
   Select one message or a subset, or replace the selection with **all owned
   messages in a completely scanned container**. Channel selection includes
   the user's replies in all its threads by default. You can rescan a single
   channel/DM without resetting the entire workspace index.
5. Click a workspace, channel, DM, thread or message link to navigate an open
   Firefox Slack tab directly to that item. For threads and messages, Slack's
   `chat.getPermalink` supplies the exact destination; links are validated
   against the selected conversation and timestamp before opening. If Slack
   rejects permalink lookup, no guess is silently presented as exact.
6. Enter the exact number of selected messages to create a paused deletion
   batch. In an explicitly unlocked test build, press **Resume** to begin.
   Ownership and account context are checked again before each deletion, and
   checkpoints precede destructive requests.
7. Interrupt freely. Firefox shutdown or a failed request never automatically
   resumes deletion; unknown outcomes are not retried blindly.

Scanning covers messages *accessible to your current account*, subject to
Slack's retention, visibility, endpoint permissions and rate limits. A
container showing **Incomplete** is never eligible for container-wide Select
All, although individually reviewed messages may still be selected.

Session credentials live in Firefox `storage.session`; review metadata and
paused deletion jobs are retained in `storage.local`. A tab-hosted extension
may continue working while its sidebar is closed, but Firefox must stay open.

## Build and verification

See [Build and test instructions](docs/BUILD_AND_TEST.md). CI compiles the Rust source and loads the WebAssembly UI in headless Firefox.
Neither success in CI nor a browser smoke test verifies Slack discovery or the deletion path. Live deletion must first be validated on a disposable message in a
workspace where you have permission to delete that message.

## Structure

- `crates/cleaner-core`: validated IDs, selection, filtering, paused queues.
- `crates/cleaner-ui`: Rust Leptos UI and thin browser/Slack adapters.
- `extension`: Firefox manifest and Trunk HTML/CSS inputs.
- `scripts/build.py`: full build and package entry point.
- `scripts/static_check.py`: offline structural safety checks.
- `AGENTS.md` / `docs/CONVENTIONS.md`: project development conventions.

No vendor backend, app installation, hidden telemetry, or credential export.
