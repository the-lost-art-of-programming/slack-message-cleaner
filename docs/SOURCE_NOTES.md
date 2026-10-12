# Source references and verification notes

The upstream [`praxis-proxy/conventions`](https://github.com/praxis-proxy/conventions)
repository is a reusable **Rust development conventions** template, not an
extension framework. It is Apache-2.0 licensed. Public GitHub views were
reviewed on 2026-10-11; the runtime could not clone the entire repository.

## Primary upstream sources

- [`docs/conventions.md`](https://github.com/praxis-proxy/conventions/blob/main/docs/conventions.md)
- [`docs/development.md`](https://github.com/praxis-proxy/conventions/blob/main/docs/development.md)
- [`.claude/CLAUDE.md`](https://github.com/praxis-proxy/conventions/blob/main/.claude/CLAUDE.md)
- [`Cargo.toml`](https://github.com/praxis-proxy/conventions/blob/main/Cargo.toml)
- [`clippy.toml`](https://github.com/praxis-proxy/conventions/blob/main/clippy.toml)
- [`deny.toml`](https://github.com/praxis-proxy/conventions/blob/main/deny.toml)
- [`rustfmt.toml`](https://github.com/praxis-proxy/conventions/blob/main/rustfmt.toml)
- [`rust-toolchain.toml`](https://github.com/praxis-proxy/conventions/blob/main/rust-toolchain.toml)
- [`.cargo/config.toml`](https://github.com/praxis-proxy/conventions/blob/main/.cargo/config.toml)
- [`Makefile`](https://github.com/praxis-proxy/conventions/blob/main/Makefile)
- [`CONTRIBUTING.md`](https://github.com/praxis-proxy/conventions/blob/main/CONTRIBUTING.md)
- [`docs/release.md`](https://github.com/praxis-proxy/conventions/blob/main/docs/release.md)
- [`.taplo.toml`](https://github.com/praxis-proxy/conventions/blob/main/.taplo.toml)
- [`typos.toml`](https://github.com/praxis-proxy/conventions/blob/main/typos.toml)
- [`.markdownlint.yaml`](https://github.com/praxis-proxy/conventions/blob/main/.markdownlint.yaml)
- [`.github/workflows/coverage.yaml`](https://github.com/praxis-proxy/conventions/blob/main/.github/workflows/coverage.yaml)
- [`.github/workflows/mutants.yaml`](https://github.com/praxis-proxy/conventions/blob/main/.github/workflows/mutants.yaml)
- [`.github/workflows/supply-chain.yaml`](https://github.com/praxis-proxy/conventions/blob/main/.github/workflows/supply-chain.yaml)
- [`.github/workflows/pr-conventions.yaml`](https://github.com/praxis-proxy/conventions/blob/main/.github/workflows/pr-conventions.yaml)

Other known workflow pages reviewed include `msrv.yaml`, `semver.yaml`,
`automated-review.yaml`, `pr-housekeeping.yaml`, `issue-triage.yaml`, and
`publish-dry-run.yaml`. The organizational issue triage rules are not
project coding conventions, and server-container workflows are not applicable.

## Important verification distinctions

The upstream root did *not* list `AGENTS.md` or `audit.toml`. The first is
created in this project, and the second is replaced by `deny.toml` and
`cargo audit` commands. `Cargo.toml` centrally declares lint rules. GitHub
raw and HTML snapshots sometimes differed in dependency patch versions;
versions chosen here reflect the newer HTML view, and need checking during
the first actual build. There are no Rust crates at this stage; we have only
validated these preparatory TOML documents parse, not compile.
