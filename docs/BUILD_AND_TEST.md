# Building and testing the Rust Firefox extension

## Current status

**Source implementation, not a verified executable build.** The authoring runtime
has no Rust toolchain, Trunk, or Firefox, and external binary installation cannot
resolve DNS. The offline Python structural checks run; Rust compilation,
Leptos/WASM generation, Firefox loading, and authenticated Slack behavior are
**not** verified here. Do not load this as a proven working deletion utility.

## Local prerequisites

1. Install Rust 1.96 with `rustup` and target `wasm32-unknown-unknown`.
2. Install Trunk (`cargo install --locked trunk`).
3. Install Firefox Developer Edition or Firefox 128+.
4. From the repository root, run:

```sh
python3 scripts/static_check.py
cargo test -p cleaner-core
cargo test -p cleaner-ui
cargo clippy -p cleaner-core --all-targets -- -D warnings
cargo check -p cleaner-ui --target wasm32-unknown-unknown
python3 scripts/build.py
```

`build.py` runs core tests and builds `dist/` using Trunk, adds `manifest.json`,
and produces `own-message-cleaner-firefox.zip`. The default build has a
**live deletion compile-time safety lock**. Only after verifying auth, ownership
and endpoints in a disposable test workspace should a tester intentionally run
`python3 scripts/build.py --enable-live-delete`. Generated JavaScript is
expected in `dist/` **only**; no handwritten JavaScript or TypeScript is used.

Open `about:debugging#/runtime/this-firefox`, select **Load Temporary Add-on**,
and choose `dist/manifest.json`. Open the **Own Message Cleaner** sidebar
through Firefox's sidebar controls. Reload a logged-in Slack tab while the
sidebar is open to capture the token, then select **Connect / Restore**.

## Browser and real-Slack acceptance tests still required

- Confirm `webRequest` catches form-data and raw-body tokens; check `auth.test`
  on a workspace that forbids installing Slack apps.
- Confirm read APIs and exact reply paging, including full archive / cursor,
  restricted channels, non-members, Slack Free retention boundaries, and
  expired session behavior.
- Verify that the packaged WASM runs under Firefox MV3's CSP.
- Check that browser `storage.session` clears credentials on restart, while
  local review/job records remain, and no job resumes by itself.
- Test a **single disposable message** in a workspace the tester controls,
  after manually verifying the original author and selecting exactly one.
- Test closed sidebar with a standalone extension tab, cancellation,
  denied deletion, HTTP 429, and interrupted in-flight checkpoints.
- Headless WebDriver tests require actual Firefox/geckodriver plus a build.
  A headless Chromium smoke check is **not** equivalent to Firefox validation.
  After building, run `python3 scripts/firefox_smoke.py` with Selenium,
  Firefox and geckodriver installed. This tests extension installation and
  Leptos UI mounting, not access to a live Slack workspace.

## Runtime limits

The runner uses the sidebar or optional standalone extension tab as the job
host. The tab must remain open for a job to run while the sidebar is closed.
Closing Firefox or the job tab stops the runner. Restoring any job always
requires the user's explicit Resume click. Unknown outcomes are excluded from
automatic retries.

No official Slack app or admin token is required for the intended auth path.
This does **not** bypass workspace policies, endpoint denial, or member
permissions. Sessions that cannot call the necessary endpoints will fail
safely, not silently work around the restriction.

Review snapshots are stored locally and contain bounded message previews;
`browser.storage.session` holds the captured token only in memory. Delete
local browser extension data to discard the index and checkpoint history.
