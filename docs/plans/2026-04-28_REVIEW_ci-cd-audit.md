# 2026-04-28 — CI/CD audit and CLAUDE.md expansion

| | |
|---|---|
| **Status** | Active — audit complete, CLAUDE.md changes landed, gaps backlog awaiting triage |
| **Scope** | `optimus_prime` branch (Rust rewrite, v2.1.0-beta.5 in flight) |
| **Branch / commit** | `optimus_prime` @ `e0493ab` on fork `an-altosian/TrimGalore` |
| **Author** | Claude Code session, dhe@altoslabs.com |
| **Related** | Upstream `FelixKrueger/TrimGalore`; no existing CI/coverage/test issues open |

## Purpose

This doc is a single record of (1) the contributor-doc improvements already landed in this session, (2) a snapshot of the current CI/CD testing setup as of 2026-04-28, and (3) a prioritized backlog of improvement candidates that future sessions can pull from.

It is intentionally placed at `docs/plans/` rather than `docs/src/content/docs/`, so it is git-tracked and GitHub-renderable but does not appear on the Astro Starlight public site at <https://felixkrueger.github.io/TrimGalore/>.

## Part 1 — Changes already landed

One commit, expanding `CLAUDE.md` to cover gaps a future Claude session would otherwise have to rediscover from scratch.

### Commit `e0493ab` — `docs(claude): expand CLAUDE.md with workflows, beta context, fastqc args`

Diff: +76 / −1, `CLAUDE.md` only.

| # | Gap that was missing | Section added |
|---|---|---|
| 1 | Active release stream (v2.0.0 stable, v2.1.0-beta.5 in flight, beta.6 pre-GA fixes from nf-core review) | New `## Active release stream` section with the four v2.x-vs-v0.6.x intentional report divergences |
| 2 | All three workflows (only `ci.yml` was previously described) | New `## CI workflows` section enumerating `ci.yml`'s five jobs, `docs.yml`, and `release.yml`'s nine jobs |
| 3 | `--fastqc_args` accepted-flag list (was "curated subset" with no list) | Table of nine accepted flags appended to `## Bundled FastQC dependency` |
| 4 | Test-fixtures inventory (was incomplete) | Table replacing the prose paragraph; adds `multi_adapters.fa`, `4_seqs_with_Ns.fastq.gz`, the `nextera_100K` / `smallRNA_100K` golden-reference fixtures, paired `polyAT`, `illumina_10K`, `illumina10K_with_polyA`, `10K_150bp` |
| 5 | Astro Starlight docs site under `docs/` (entirely unmentioned) | New `## Documentation site` section with the npm dev workflow and the `CHANGELOG.md` mirror requirement |
| 6 | `Dockerfile` and multi-arch GHCR distribution | New `## Distribution` section (crates.io / bioconda / Docker / prebuilt binaries) |
| 7 | Pure-Rust gzip stack (`zlib-rs` + `deflate_rust`) | Added to `## Conventions worth knowing` |
| 8 | Specific-file `git add` discipline | Added to `## Conventions worth knowing` |

### What was deliberately NOT added

These observations exist but did not warrant a CLAUDE.md edit, on the grounds that "don't add obvious instructions" outweighs "be exhaustive":

- Generic Rust development tips (covered by global user rules).
- Complete dependency list (Cargo.toml is authoritative; only the pure-Rust gzip choice is non-obvious enough to call out).
- Repetition of the `src/` module map (existing description matches the file layout exactly).
- "Always run `cargo test` before push" (already in the build section).

## Part 2 — Current CI/CD testing setup snapshot

### Workflows at a glance

| Workflow | Trigger | Jobs | Wall-clock |
|---|---|---|---|
| `ci.yml` | push to `master`/`dev`/`optimus_prime`; PRs targeting `master`/`optimus_prime`; Mondays 06:00 UTC for `audit` only | 5 jobs (one schedule-skipped exception) | ~5–10 min |
| `docs.yml` | push to `master`/`optimus_prime` only when `docs/**`, `CHANGELOG.md`, or this workflow itself changed; or `workflow_dispatch` | `build` → `deploy` (Astro Starlight → GitHub Pages) | ~1–2 min |
| `release.yml` | push to `optimus_prime`/`master`; `workflow_dispatch` with `dry_run: true` option | 9 jobs, three of them matrix builds | ~10–15 min when releasing |

### `ci.yml` job breakdown

#### `rust-tests`

- `cargo test` (debug profile).
- `cargo build --release`.
- Content-targeted regex on `--version` long form: `^[0-9a-f]{7,40} — [a-z0-9_]+/[a-z0-9_]+ — built [0-9T:Z-]+$` (literal em-dash, ISO-8601 UTC).
- Negative grep: `-V` short form must NOT carry the provenance line.

#### `reproducibility`

- Own cargo cache key prefix (`cargo-repro-`) so this job's `cargo clean` does not invalidate `rust-tests`'s `target/`.
- Two release builds with fixed `SOURCE_DATE_EPOCH=1700000000`; sha256-diff.
- The shell `export` is deliberate (the `VAR=x cmd1 && cmd2` form would only scope the var to `cmd1`).
- Negative test: `SOURCE_DATE_EPOCH=garbage cargo build` must hard-fail with output mentioning the var name; defends against silent wall-clock fallback.

#### `lint`

- `cargo fmt --all -- --check`.
- `cargo clippy --all-targets --release -- -D warnings`.
- The `--release` flag for clippy is unusual; the project treats release as the canonical build because of `lto = true` and `codegen-units = 1` in `Cargo.toml`.

#### `audit`

- `rustsec/audit-check@v2.0.0` (action SHA pinned).
- The only job that runs on the weekly cron.
- Permission set is `{ contents: read, checks: write, issues: write }` so it can open issues for new advisories.

#### `validation`

Gated by `needs: [rust-tests, lint]` so it does not start until those pass.

Installs Perl Trim Galore 0.6.11 from a pinned upstream raw-GitHub URL plus Cutadapt 5.2 via conda, then runs **17 distinct validation steps**:

| Category | Steps |
|---|---|
| md5 byte-identity vs Perl 0.6.11 | SE, PE (2 files), hardtrim5, clock (2 files), demux (4 files, with `--no_poly_g` to disable v2.0 auto poly-G so the comparison is apples-to-apples) |
| Multi-pair regression (the v2.x widening) | 3-pair PE (`--paired -j 4`), 2-pair `--clock`, 2-pair `--implicon`, 2-pair `--cores 1` (sequential code path), `--retain_unpaired × multi-pair` |
| Adapter auto-detection per pair | Asserts the log emits 2 `Auto-detecting adapter type` lines for 2 pairs (the v2.x deviation from Perl) |
| Negative tests (non-zero exit + specific grep + no partial output) | Odd-count input rejection, R1==R2 rejection, `--basename` with multi-pair, missing input mid-list, output-collision pre-flight (vanilla + case-only aliases for issue #216), `--clock` duplicate-pair, `--clock` odd-count |
| Bundled FastQC smoke | `! command -v fastqc`, then asserts canonical `*_fastqc.html` + `*_fastqc.zip` produced and the zip contains `summary.txt`, `fastqc_data.txt`, `Images/`, `Icons/` |

### `release.yml` pipeline

```text
check-release  ─┬─►  build-binaries (matrix: linux-x86_64, linux-aarch64, macos-aarch64)
                │      └─► smoke-test-binaries (linux-x86_64 only)
                │
                ├─►  docker-build (matrix: linux/amd64, linux/arm64 — NATIVE arm64 runners)
                │      └─► docker-merge (multi-arch manifest)
                │            └─► smoke-test-docker
                │
                └─►  create-tag-and-release  ─►  upload-binaries  ─►  publish-crate
```

Notable design choices:

- `check-release` enforces a branch ↔ pre-release policy: GA only from `master`, pre-release only from `optimus_prime`.
- Cross-version drift guard: `Cargo.lock`'s `trim-galore` version must match `Cargo.toml`'s.
- Tag creation lives inside the workflow, not on the developer's laptop (the workflow header explicitly says "Never push `v*` tags manually").
- `workflow_dispatch` with `dry_run: true` rehearses the full pipeline (build + manifest preview) without publishing to crates.io, pushing tags, or pushing Docker images.
- Native arm64 runners (`ubuntu-24.04-arm`) for both binary and Docker builds — no QEMU emulation tax.
- Trusted publishing: `rust-lang/crates-io-auth-action@v1.0.4` with `id-token: write` permission uses OIDC instead of a long-lived `CARGO_REGISTRY_TOKEN`.
- `cargo publish --no-verify` skips the redundant verify build at the end.

### Where the unit tests live

165 `#[test]` functions across 11 source files, all in-tree `#[cfg(test)]` modules. There are no `tests/`, `benches/`, or `examples/` directories at the crate root.

| File | Tests | Coverage area |
|---|---:|---|
| `quality.rs` | 38 | BWA-style 3' quality trimming algorithm |
| `adapter.rs` | 32 | Built-in adapters + auto-detection |
| `cli.rs` | 27 | Clap config + `Cli::validate()` (the only file that touches `test_files/`) |
| `trimmer.rs` | 13 | Pipeline orchestrator |
| `report.rs` | 13 | Trimming reports |
| `alignment.rs` | 13 | Semi-global DP |
| `fastqc.rs` | 9 | FastQC integration |
| `fastq.rs` | 9 | FASTQ I/O |
| `filters.rs` | 8 | Length/max-N/unpaired-rescue filters |
| `specialty.rs` | 7 | hardtrim/clock/implicon |
| `io.rs` | 7 | Output naming |
| **`parallel.rs`** | **0** | Worker pool — gap |
| **`demux.rs`** | **0** | 3' inline barcode demux — gap |
| `main.rs` | 0 | Thin dispatcher (acceptable) |
| `lib.rs` | 0 | Re-export module (acceptable) |

Only `cli.rs` reads from `test_files/` — every other module's tests use synthetic in-memory data.

This makes `cargo test` fast (no multi-MB gzip round trips), but it also means most module-level tests cannot catch fixture-format regressions; those only surface in `cli.rs` tests or in the CI `validation` job's md5 comparison.

## Part 3 — Strengths of the current setup

Worth noting these explicitly so future sessions do not "improve" them away:

- **All action SHAs are pinned** (`actions/checkout@de0fac2e...` not `@v6`); supply-chain hardened against tag-rewrite attacks.
- **Validation tests negative paths as rigorously as positive paths** — every error message is part of the contract because parsers (Snakemake, Nextflow, nf-core) grep for it.
- **Reproducibility includes a hard-fail-on-malformed-input test**, not just a "two builds match" test; defends against the silent-fallback regression class.
- **Pre-release / GA policy enforced in CI**, not just convention.
- **Per-job cache prefixes** prevent the reproducibility job's `cargo clean` from invalidating the test job's `target/`.
- **Trusted publishing (OIDC)** rather than a `CARGO_REGISTRY_TOKEN` secret.
- **Native arm64 runners** rather than QEMU-emulated cross-builds.

## Part 4 — Gaps and improvement candidates

Prioritized for value-per-effort.  Effort estimates assume a focused session by someone familiar with the codebase.

| # | Gap | Impact | Effort | Notes |
|--:|---|---|---|---|
| 1 | `parallel.rs` has zero unit tests | Worker-pool concurrency surface; only exercised by CI's md5 validation. A worker-ordering or chunk-boundary bug surfaces only there | Medium | Needs synthetic FASTQ generation in-test; chunk ordering, gzip-member concatenation correctness, error propagation when a worker panics are the obvious targets |
| 2 | `demux.rs` has zero unit tests | Pure barcode-matching logic; only validated via Perl-md5 comparison. Local breakage is invisible | Easy | Lowest-hanging test fruit — pure functions, easy synthetic input |
| 3 | No macOS runner in `ci.yml` | Cross-platform regressions surface only at release-build time on `macos-latest` | Easy | Add `matrix.os: [ubuntu-latest, macos-latest]` to `rust-tests`; ~2× wall-clock |
| 4 | `rust-tests` runs `cargo test` (debug only) | `Cargo.toml` has `lto = true, codegen-units = 1`; CLAUDE.md notes release builds "behave subtly differently from debug" | Easy | Add `cargo test --release` step; slower CI but catches LTO-only bugs |
| 5 | No coverage reporting | With 165 unit tests, `cargo-tarpaulin` or `cargo-llvm-cov` would tell you where new tests have the highest marginal value (almost certainly `parallel.rs`, `demux.rs`) | Easy | One new job; slower runs; useful artifact (HTML report) |
| 6 | `validation` is one big serial job (17 steps) | Conda install + cargo build amortized across 17 steps, but the steps themselves are sequential. Could fan out for faster feedback | Medium | Needs careful matrix design and conda caching to avoid repeated installs killing the win |
| 7 | No scheduled dry-run of `release.yml` | Packaging bugs surface only at real release time | Easy | Add `schedule:` trigger that forces `dry_run: true` (e.g., weekly); re-uses the existing dry-run path |
| 8 | No nf-core / MultiQC integration matrix in CI | The CHANGELOG explicitly mentions nf-core pre-GA validation surfacing the beta-5 bugs; that's currently human-driven | Hard | Requires Nextflow runner, more wall-clock; would catch a real class of integration bug earlier |
| 9 | `audit-check` permissions include `issues: write` | Posts new advisories as GitHub issues; on a fork (e.g., `an-altosian/TrimGalore`) this could spam the issue tracker | Trivial | Drop the permission on the fork only |
| 10 | `cargo publish --no-verify` | Skips the redundant build verify before crates.io publish; mitigated by the smoke tests, but a manifest-only error class is not caught | Trivial | Remove `--no-verify` (slower release; +5 min) |

### Recommended top-3 to act on first

1. **#2 — unit tests for `demux.rs`**.  Highest value-per-effort: pure functions, easy to test, fills the most surprising 0-test gap.
2. **#1 — unit tests for `parallel.rs`**.  Most concurrency-prone module in the crate, currently invisible to local feedback.
3. **#5 — coverage reporting**.  Cheap to add, and immediately identifies which other modules' coverage is lower than the headline test count would suggest (the 165-tests-across-11-files distribution is likely uneven once you account for branch coverage).

### Out of scope for this audit

These were noticed but are tangential to CI/CD testing:

- Stale comment in `Cargo.toml:38` referencing `/Users/fkrueger/.claude/plans/bundled-fastqc-rust.md` (a path on Felix's local machine that does not exist in this repo).
- `dependabot.yml` configuration — present but not reviewed for coverage of the npm sub-package under `docs/`.
- Whether the `audit` job's GitHub-issue auto-filing has been triggered in practice.

## Appendix — references

| | |
|---|---|
| Workflow files | `.github/workflows/ci.yml`, `.github/workflows/docs.yml`, `.github/workflows/release.yml` |
| CLAUDE.md commit | `e0493ab` on `optimus_prime` |
| Fork URL | <https://github.com/an-altosian/TrimGalore> |
| Upstream URL | <https://github.com/FelixKrueger/TrimGalore> |
| CHANGELOG (canonical, with "Unreleased" block for beta.6) | `CHANGELOG.md` |
| Mirrored docs-site changelog (must stay in sync) | `docs/src/content/docs/reference/changelog.md` |
