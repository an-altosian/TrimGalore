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
| 4 | Test-fixtures inventory (was incomplete) | Table replacing the prose paragraph; adds `multi_adapters.fa`, `4_seqs_with_Ns.fastq.gz`, the `nextera_100K` / `smallRNA_100K` adapter-detection fixtures, paired `polyAT`, `illumina_10K`, `illumina10K_with_polyA`, `10K_150bp`. **Caveat — found later in this same audit (see Part 4 #11)**: the CLAUDE.md change calls the `*_trimming_report.txt` files alongside `nextera_100K` / `smallRNA_100K` "golden references", but they are not actually referenced by any test or CI step. CLAUDE.md will need a follow-up edit when the orphan files are either wired up or removed. |
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

### Regression-test layering

The repo has regression tests, but at five distinct layers — and one of them is non-functional.

#### Layer 1 — Named regression unit tests in `src/`

Two explicit ones, both very recent and tied to the nf-core pre-GA review:

| Issue | File | Tests | What it locks down |
|---|---|---|---|
| #232 | `src/trimmer.rs:754-790` | `test_bp_after_cutadapt_skips_rrbs_truncation` + `test_bp_after_cutadapt_equals_seq_len_when_no_rrbs` | RRBS `Total written (filtered)` bp accounting — `bp_after_cutadapt` captured pre-RRBS-truncation |
| #233 | `src/report.rs:1572-1620` | `test_run_footer_emits_zero_count_filter_lines` + `test_pair_validation_emits_zero_n_count_line` | `RUN STATISTICS` lines emitted even at count 0; protects MultiQC's canonical fallback parser |

Both follow a section-header convention: `// ── #NNN regression: short description ──` immediately above the test, with the test body linking to the upstream issue.

Two further regression-flavoured tests are present but less prominently labelled:

- `src/adapter.rs:767` — temp-dir name `tg_test_autodetect_illumina_regression` flags the test as a regression for an adapter auto-detection bug.
- `src/cli.rs:610` — comment "Replaces the earlier 'strict-2' regression guard"; the test was kept after the underlying logic was rewritten.

Plus three explicit unit tests for the `--trim-n × --rrbs` interaction (Perl v0.6.x parity) at `src/trimmer.rs:715-742` (`test_trim_n_without_rrbs_trims_trailing_ns`, `test_trim_n_with_rrbs_suppressed`, `test_trim_n_rrbs_also_preserves_leading_ns`).

#### Layer 2 — Regression-guard steps in the CI `validation` job

Nine of the seventeen validation steps are explicitly regression-flavoured:

| Step | Tied to |
|---|---|
| `Validate multi-pair paired-end (regression guard — beta-1 reporter scenario)` | beta-1 bug with 3+ pairs |
| `Validate paired-end rejects odd-count input` | Negative-path: `--paired` with odd file count |
| `Validate paired-end rejects R1==R2` | Same file twice rejected |
| `Validate --basename rejected with multi-pair` | `--basename` meaningless with >1 pair |
| `Validate multi-pair --clock (regression guard for the v2.x widening)` | When `--clock` was widened in v2.x to accept multi-pair |
| `Validate --clock duplicate-pair rejection (regression guard)` | Duplicate pairs in multi-pair `--clock` |
| `Validate --clock odd-count rejection (regression guard)` | Odd file count in `--clock` (different code path from `--paired`) |
| `Validate paired-end collision pre-flight catches case-only aliases (issue #216)` | APFS/NTFS case-only-alias silent-overwrite |
| `Validate missing input mid-list fails fast` | Pair B's R1 missing must abort *before* pair A starts |

Each asserts not only non-zero exit but also a specific error message via grep AND that no partial output files were written. The error message text itself is a contract.

#### Layer 3 — Reference-implementation oracle tests

Every md5-byte-identity step in the validation job is a regression test against an external reference: `Validate single-end output`, `Validate paired-end output`, `Validate hardtrim5`, `Validate clock mode`, `Validate demux` all md5-compare against Perl Trim Galore 0.6.11. This is the **primary** regression mechanism for the rewrite — the entire "faithful Rust port" claim rests on these.

#### Layer 4 — Reproducibility regression tests

The `reproducibility` job is itself a regression test against the build pipeline:

- Build twice with `SOURCE_DATE_EPOCH=1700000000` → must be sha256-identical.
- Build with `SOURCE_DATE_EPOCH=garbage` → must hard-fail with the var name in the error output (a meta-regression test against silent wall-clock fallback).

#### Layer 5 — Golden-reference fixtures (committed but ORPHANED)

Four files in `test_files/` look like golden snapshots:

```text
test_files/nextera_100K.fastq.gz_trimming_report.txt
test_files/smallRNA_100K.fastq.gz_trimming_report.txt
test_files/smallRNA_100K_R1.fastq.gz_trimming_report.txt   # ← orphan — no R1 fastq.gz exists
test_files/smallRNA_100K_R2.fastq.gz_trimming_report.txt
```

But:

- No test in `src/` references them (`grep -rn 'nextera_100K\|smallRNA_100K' src/` returns zero hits).
- No CI step references them.
- `smallRNA_100K_R1.fastq.gz` doesn't exist — only its `_trimming_report.txt` does.

So these are committed *as if* they were regression oracles, but nothing actually compares against them. Almost certainly leftovers from an earlier comparison harness that was either removed or never wired up. They look like regression infrastructure but are not functioning as such; this is reflected in Part 4 #11 below.

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
| 11 | Four `*_trimming_report.txt` files in `test_files/` look like golden references but are unused | "Snapshot regression coverage" appears to exist but does not function. CLAUDE.md (and the original audit-doc draft) called them golden references — that overclaim is now corrected in Part 1. Misleading to future readers until resolved | Easy | Either wire them into a snapshot test, or delete them (and the fully-orphaned `smallRNA_100K_R1.fastq.gz_trimming_report.txt`). See Part 5 §5.5/§5.6 for related candidates that would re-use this infrastructure |
| 12 | No snapshot-test crate (`insta`, `expect-test`, `goldenfile`) | When the team needs more snapshot-style coverage they hand-roll it; the orphan in #11 is plausibly a casualty of that. Several Part 5 candidates would benefit (#5.4 trimming-report text drift, follow-up coverage of #232/#233 wider report shapes) | Easy | Adopt `insta` (most widely used in Rust projects); reduces friction for the wave of bug-derived tests in Part 5 |
| 13 | `// ── #NNN regression: ──` convention applied to only 2 issues out of the bug-fix list | Excellent pattern, dramatically underused. Not greppable as a project-wide convention until adopted broadly | Trivial (per existing test) | Adopt as project-wide. Backfill on the existing tests that secretly are regression tests (`tg_test_autodetect_illumina_regression`, the `--trim-n × --rrbs` block at `src/trimmer.rs:715-742`); ensure new bug fixes use it |

### Recommended top-3 to act on first

1. **#2 — unit tests for `demux.rs`**.  Highest value-per-effort: pure functions, easy to test, fills the most surprising 0-test gap.
2. **#1 — unit tests for `parallel.rs`**.  Most concurrency-prone module in the crate, currently invisible to local feedback.
3. **#5 — coverage reporting**.  Cheap to add, and immediately identifies which other modules' coverage is lower than the headline test count would suggest (the 165-tests-across-11-files distribution is likely uneven once you account for branch coverage).

### Out of scope for this audit

These were noticed but are tangential to CI/CD testing:

- Stale comment in `Cargo.toml:38` referencing `/Users/fkrueger/.claude/plans/bundled-fastqc-rust.md` (a path on Felix's local machine that does not exist in this repo).
- `dependabot.yml` configuration — present but not reviewed for coverage of the npm sub-package under `docs/`.
- Whether the `audit` job's GitHub-issue auto-filing has been triggered in practice.

## Part 5 — Bug-derived regression test candidates

Cross-referencing every bug fix in `CHANGELOG.md` (since v2.0.0) against existing `#[test]` functions and CI steps surfaces a tier of bugs whose fixes shipped without an accompanying regression test.
Each candidate below names the bug, its source (commit / issue), why a regression test is worth writing now, where to put it, and a concrete test plan.

Ordered most-valuable first.

### 5.1 — Multi-member gzip FASTQ decoding

| | |
|---|---|
| Source | v2.0.0 commit `9dcf519` ("Multi-member gzip FASTQ files decode correctly") |
| Why valuable | The `--cores N` parallel mode produces multi-member gzip output (each worker writes its own gzip member, concatenated in order — RFC 1952 permits this). The reader must round-trip such files. A regression here would silently truncate parallel-mode output read back by Trim Galore as a follow-on input |
| Where | `src/fastq.rs` `#[cfg(test)]` block |
| Test plan | (a) Construct two single-record gzip streams with `flate2` in-memory; concatenate the bytes. (b) Pass the concatenated buffer through `FastqReader`. (c) Assert both records are yielded with their expected sequence and quality. (d) Inverse: produce a multi-member gzip via the parallel-mode code path on synthetic input and round-trip it through the reader |
| Effort | Easy — `flate2` is already a dep; ~50 LOC test |

### 5.2 — Parallel/serial path stat-tracking parity

| | |
|---|---|
| Source | v2.0.0 commits `82d1e34`, `3996fc5` ("Parallel-path `total_bp_after_trim` and `r2_clipped_5prime` stats now tracked correctly") |
| Why valuable | Directly addresses Part 4 gap #1 (`parallel.rs` has 0 unit tests). The original bug was that `total_bp_after_trim` and `rrbs_r2_clipped_5prime` drifted between parallel and serial paths. The fix touched `src/parallel.rs:366,393,394,613` — exactly the surface that has no unit-test coverage today |
| Where | `src/parallel.rs` `#[cfg(test)]` block |
| Test plan | Property-style equivalence test: feed identical synthetic input through both `run_single_end` (with `--cores 1`, sequential path) and the parallel multi-worker path (with `--cores 4`). Assert that the resulting `TrimStats` are field-by-field equal — `total_bp_after_trim`, `rrbs_r2_clipped_5prime`, `reads_written`, the per-adapter counts. Repeat for paired-end (`run_paired_end`) |
| Effort | Medium — needs synthetic input with deterministic ordering; assertion is mechanical |

### 5.3 — Adapter auto-detection 1M-read scan limit

| | |
|---|---|
| Source | v2.0.0 commit `9129650` ("Adapter auto-detection scans exactly 1M reads") |
| Why valuable | The constant `MAX_SCAN_READS = 1_000_000` exists at `src/adapter.rs:85`, but no test asserts the limit. A regression here changes detection behaviour on >1M-read inputs in a non-obvious way (slow vs fast, or wrong call vs right) |
| Where | `src/adapter.rs` `#[cfg(test)]` block |
| Test plan | (a) Generate a synthetic 1.5M-read FASTQ stream where the first 1M reads contain Illumina adapter and the last 0.5M contain Nextera. (b) Run the detection entry point. (c) Assert the chosen adapter is Illumina (Nextera was past the scan boundary). (d) Inverse: same test with adapters swapped — must choose Nextera. (e) A cheap third test: detection on exactly 1M reads vs 1M+1 reads must yield the same result |
| Effort | Medium — large synthetic input; generate via a cycle iterator rather than materialising 1M records |

### 5.4 — Trimming-report text drift (PE param-summary "-end" suffix)

| | |
|---|---|
| Source | v2.1.0-beta.3 ("paired-end parameter-summary line previously emitted a stray `-end` suffix") |
| Why valuable | Trivial to write, prevents an entire class of typo regression. The original bug was `...before a sequence pair gets removed-end: 20 bp`. MultiQC and similar parsers grep for the exact line, so any future text drift breaks them silently. A growing test of this form (assert exact substring; assert absence of known-bad substring) becomes the unit-level twin of the CI md5 oracle |
| Where | `src/report.rs` `#[cfg(test)]` block |
| Test plan | (a) Render the SE param-summary section; assert the exact substring `"length single-end: "`. (b) Render the PE param-summary section; assert exact substring `"before a sequence pair gets removed: "` AND assert absence of `"removed-end"` anywhere in the rendered text. Optional follow-on: cover the four documented v2.x-vs-v0.6.x report divergences listed in `CHANGELOG.md` "Unreleased" — RRBS quality-trim line shape, dropped adapter family-name annotation, omitted bases-preceding-removed-adapters histogram, modern Cutadapt `max.err` formula |
| Effort | Trivial — ~20 LOC for the core test; would benefit from `insta` (Part 4 #12) for the wider follow-on |

### 5.5 — Demux CRLF barcode-file handling

| | |
|---|---|
| Source | v0.6.11 ("Fixed `--demux` handling of CR (carriage return) characters in barcode files") |
| Why valuable | Inherited semantics in v2.x. `src/demux.rs` has 0 unit tests (Part 4 gap #2). A barcode samplesheet authored on Windows (CRLF line endings) would have its barcodes appended with `\r` if not stripped — every read goes to NoCode, silent failure that only surfaces on real data |
| Where | `src/demux.rs` `#[cfg(test)]` block |
| Test plan | (a) Construct two synthetic samplesheet strings: one with `\n`, one with `\r\n` line endings, both with the same logical content. (b) Parse both via the samplesheet-loader function. (c) Assert the parsed barcode `Vec<String>` is byte-equal across the two cases. (d) Optional integration variant: run a small in-memory demux against a CRLF samplesheet and assert read routing matches the LF case |
| Effort | Easy — pure-string function |

### 5.6 — Demux NoCode routing for short reads

| | |
|---|---|
| Source | v0.6.11 ("barcode length issue with NoCode") |
| Why valuable | Inherited semantics in v2.x. `src/demux.rs:179-184` has explicit "Read too short for barcode — goes to NoCode" handling. No unit test asserts this. A regression that crashed on short reads would only surface on real data |
| Where | `src/demux.rs` `#[cfg(test)]` block |
| Test plan | (a) Construct a `FastqRecord` whose sequence is shorter than the barcode length. (b) Pass through the demux per-read function. (c) Assert it routes to the `NoCode` writer (not crash, not panic). (d) Boundary case: a record exactly equal to barcode length should NOT be NoCode |
| Effort | Easy |

### Lower-priority but cheap fills

These have weaker "would catch a real regression" signals but are quick wins:

- **`--fastqc_args` warn-and-ignore path.** `src/fastqc.rs` tests the accept path for known flags, but the warn path for unknown flags (e.g., `--made-up-flag`) is the safety net for forward-compat. A test asserting (a) a warning is logged and (b) parsing of subsequent valid flags is unaffected.
- **Output-collision pre-flight Rust unit test.** CI validates this end-to-end in Part 2 layer 2, but a unit test on the case-folded hashing logic in `src/cli.rs` (or wherever the pre-flight lives) would shorten the local feedback loop and make TDD on this surface faster.
- **`--retain_unpaired × multi-pair` filename construction.** CI validates the filename layout, but no Rust unit test asserts the per-pair `unpaired_1` / `unpaired_2` filename construction in `src/io.rs`.

### Things deliberately NOT in this list

For each, here's why a regression test is not a priority:

- **v0.6.11 `--nextseq + --rrbs` issue #210** — was a Perl bug; v2.x may or may not exhibit it. Investigation is needed to know if the bug exists in the Rust code at all *before* a regression test makes sense.
- **v2.0.0 `--fastqc_args` hyphenated values (commit `def0344`)** — `src/fastqc.rs` already covers this in 9 unit tests; verified by inspection of the parser.
- **v2.0.0 Cutadapt-section MultiQC parity (commit `eedbc66`)** — covered indirectly by the CI `validation` md5 comparison and partly by the #232 / #233 unit tests.
- **v0.6.11 / older Perl-era bugs whose fixes are in dead Perl code, not Rust.** Tecan kit incompatibility and MseI handling are documentation issues, not regression-test gaps.
- **`maxn_fraction` declaration bug (v0.6.9)** — `filters.rs` already has `test_filter_too_many_n_fraction` covering this surface.

## Appendix — references

| | |
|---|---|
| Workflow files | `.github/workflows/ci.yml`, `.github/workflows/docs.yml`, `.github/workflows/release.yml` |
| CLAUDE.md commit | `e0493ab` on `optimus_prime` |
| Fork URL | <https://github.com/an-altosian/TrimGalore> |
| Upstream URL | <https://github.com/FelixKrueger/TrimGalore> |
| CHANGELOG (canonical, with "Unreleased" block for beta.6) | `CHANGELOG.md` |
| Mirrored docs-site changelog (must stay in sync) | `docs/src/content/docs/reference/changelog.md` |
