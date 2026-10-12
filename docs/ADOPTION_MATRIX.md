# Praxis conventions audit and adoption matrix

Reviewed on 2026-10-11 against the public repository:
https://github.com/praxis-proxy/conventions

**Verification limits:** repository pages and named file contents were examined
through GitHub's public views. The source repository could not be cloned in the
working runtime, so this is NOT a claim that every nested file or historical
revision was independently enumerated. Some raw GitHub mirrors appeared to
lag the current HTML views; current HTML views were preferred. We also did not
run Cargo/Clippy because Rust tooling is unavailable in this runtime.

## Source findings and decisions

| Upstream source | Finding | Our disposition |
| --- | --- | --- |
| `README.md` | Strict Rust template; probe crate; quality gates | Adapt |
| `docs/conventions.md` | Small pure functions; no panic/casts; tests, docs, reviewability | Adopt |
| `docs/development.md` | Rust 1.96, nightly fmt, audit/coverage/tooling | Adapt for WASM |
| `docs/release.md` | SemVer, changelog and pre-release gates | Adapt for Firefox zip/XPI |
| `.claude/CLAUDE.md` | Agent commands and references | Translate into `AGENTS.md` |
| `Cargo.toml` | Rust 2024, workspace dependencies, >100 deny-level lints | Adopt with browser exceptions |
| `rust-toolchain.toml` | Stable 1.96.0; Clippy and rust-analyzer | Adopt + wasm target |
| `rustfmt.toml` | Nightly options, 120-char Rust width | Adopt |
| `clippy.toml` | Tight complexity/function/argument limits | Adapt server restrictions |
| `.cargo/config.toml` | `rustdocflags = ["-D", "warnings"]` | Adopt |
| `deny.toml` | Audit policy, license allowlist, pinned sources | Adapt to UI dependencies |
| `.taplo.toml` | TOML scope, preserve key order | Adopt |
| `typos.toml` | Excludes Cargo.lock and target | Adopt |
| `.markdownlint.yaml` | 80-column prose, named code fences | Adopt |
| `.gitattributes`, `.gitignore` | Source/repo hygiene | Adapt in Rust transition |
| `.hooks/pre-commit` | Signed-commit precondition, make lint | Optional local adaptation |
| `Makefile` | Build/format/lint/test/coverage/audit commands | Adapt, remove server/KIND |
| `CONTRIBUTING.md` | Human review, strict PR gates | Adopt principles |
| `SECURITY.md` | Reporting and supported-version process | Adapt to credentials/extension |
| `MAINTAINERS.md` | Praxis organizational roles | Do not copy people |
| `CODE_OF_CONDUCT.md` | Community conduct standard | Adopt on publication as needed |
| `Containerfile`, `hack/*` | Server/container/KIND infrastructure | Not applicable |
| `crates/conventions-probe/*` | Quality-gate exercise crate | Replace with core/browser crates |
| `.github/workflows/coverage.yaml` | Host coverage gate | Adopt for Rust core |
| `.github/workflows/mutants.yaml` | Weekly mutation testing | Adopt for Rust core |
| `.github/workflows/supply-chain.yaml` | Scheduled cargo audit + deny | Adopt |
| `.github/workflows/msrv.yaml` | Minimum Rust version tests | Adopt |
| `.github/workflows/semver.yaml` | API/version compatibility check | Adapt |
| `.github/workflows/pr-conventions.yaml` | Change size, commits, DCO, human review | Adapt |
| `.github/workflows/pr-housekeeping.yaml` | Draft/old PR cleanup | Optional |
| `.github/workflows/issue-triage.yaml` | Org-specific issue rules | Optional |
| `.github/workflows/automated-review.yaml` | Cloud review integration | Exclude by default |
| `.github/workflows/publish-dry-run.yaml` | Crate packaging | Replace with extension packaging |
| `.github/CODEOWNERS` | Placeholder maintainers | Set on repo creation |

## Exact-name check

The public repository root listing does **not** show `AGENTS.md` or
`audit.toml`, and no `lenses/` directory appears at root. The applicable
strict lint declarations are in **root `Cargo.toml`** under
`[workspace.lints]`; supply-chain policy is in **`deny.toml`**; vulnerability
checks are invoked by **`cargo audit`** in `Makefile` and CI. Its agent
instructions are at **`.claude/CLAUDE.md`**. Therefore this project defines
its own top-level `AGENTS.md` rather than representing it as a copied file.

## Coverage and critical-path tests

Pure core coverage >=90% line / >=80% region, plus mutation testing.
Additional safety gates for the Firefox extension: owner-only deletion,
workspace isolation, explicit confirmation, immutable batch scopes, fault
handling, Retry-After, interruptions, uncertain-result reconciliation,
and no credentials in logs/storage/packaged files.

## Intentional exemptions (review on implementation)

1. Async futures in browser/WASM adapters may be non-Send.
2. Slack API envelopes may include unknown fields even if owned configs are
   strictly deserialized.
3. DOM-specific code cannot be validated solely with native cargo tests.
4. Non-server project does not need Tokio, `std::io::stdin` restrictions,
   containers, Kubernetes, or crates.io release tasks.
5. Upstream CI may require modifications before sharing reusable workflows,
   especially MSRV, wasm target, artifact packaging, and least privilege.

## Known non-completion

This preparatory stage does not constitute a complete clone/mirror or a
successful run of upstream's complete CI. `Cargo.toml` is an empty workspace
with documented lints to fill before implementing the first Rust crate.
