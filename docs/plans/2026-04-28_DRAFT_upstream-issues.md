# 2026-04-28 — Draft upstream issues

| | |
|---|---|
| **Status** | **FILED 2026-04-28** — 7 issues live on FelixKrueger/TrimGalore (issues #242 – #248) |
| **Target repo** | `FelixKrueger/TrimGalore` |
| **Filer** | `an-altosian` (via `gh issue create`) |
| **Source** | Parity-hunt findings + CI/test audit + performance audit on `optimus_prime` |
| **Total** | 7 issues — 3 bug, 1 discussion, 2 tracking, 1 performance |

## Filed issues — quick links

| # | Title (truncated) | Type | Upstream URL |
|---|---|---|---|
| 1 | `--max_n 0.5` (fractional) silently ignored | BUG/HIGH | <https://github.com/FelixKrueger/TrimGalore/issues/243> |
| 2 | `--clip_r1` lowercase rejected | BUG/HIGH | <https://github.com/FelixKrueger/TrimGalore/issues/242> |
| 3 | `--basename` PE filename pattern | BUG/HIGH | <https://github.com/FelixKrueger/TrimGalore/issues/244> |
| 4 | Three behavioural divergences for triage | DISCUSSION | <https://github.com/FelixKrueger/TrimGalore/issues/245> |
| 5 | Test coverage gaps + bug-derived candidates | TRACKING/TESTS | <https://github.com/FelixKrueger/TrimGalore/issues/246> |
| 6 | CI / test-infrastructure improvements | TRACKING/CI | <https://github.com/FelixKrueger/TrimGalore/issues/247> |
| 7 | Profiling report: gzip = 60.7% of CPU + 3 quick wins | PERF | <https://github.com/FelixKrueger/TrimGalore/issues/248> |

## Comments posted on filed issues (post-filing additions)

| Issue | Comment | Why |
|---|---|---|
| #245 | [P3-F3 (`--clock`/`--implicon` imply `--paired` in v2.x) added as item D](https://github.com/FelixKrueger/TrimGalore/issues/245#issuecomment-4338501630) | Surfaced while extending the parity-hunt harness; same "needs classification" bucket as the original 3 items |
| #246 | [Extended-coverage update](https://github.com/FelixKrueger/TrimGalore/issues/246#issuecomment-4338537192) | Harness now covers 19 flag paths; 2 of 6 Phase-5 candidates partially exercised; one new gap (demux barcode-matching) flagged |

The body text below is preserved verbatim as filed (with the `**Filed at:**` placeholders now resolved to the URLs above).

---

## Issue 1 — `[BUG | HIGH]` `--max_n 0.5` (fractional) silently ignored — outputs as if no filtering applied

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/243

**Body:**

`--max_n` accepts a value between 0 and 1 as "discard reads where the fraction of N bases exceeds this threshold" (per the v0.6.8 CHANGELOG: *"the option `--max_n COUNT` now interprets value between 0 and 1 as fraction of the read length"*). The Rust implementation appears to silently treat the fractional value as integer `0` (or otherwise dispatches to the absolute-mode filter), producing output identical to `--max_n 5` — i.e., effectively no filtering on the bundled `4_seqs_with_Ns.fastq.gz` test fixture.

**Reproducer**

```bash
# Setup: Perl trim_galore from the master branch + cutadapt + Rust binary built from optimus_prime

# Perl 0.6.11 — correct fractional behaviour
trim_galore_perl --max_n 0.5 -o /tmp/perl_out test_files/4_seqs_with_Ns.fastq.gz
md5sum <(gzip -dc /tmp/perl_out/4_seqs_with_Ns_trimmed.fq.gz)
# c83ad7ed2decedc795b993f74333758c   (2 records pass — those without high-N fraction)

# Rust v2.1.0-beta.5 — fractional ignored
./target/release/trim_galore --max_n 0.5 -o /tmp/rust_out test_files/4_seqs_with_Ns.fastq.gz
md5sum <(gzip -dc /tmp/rust_out/4_seqs_with_Ns_trimmed.fq.gz)
# bb1bc6f3799ddf48e534d09e44e04a4c   (4 records — same as --max_n 5)
```

**Hypothesis on the fix location**

The filter logic itself looks present: `src/filters.rs` already has a `MaxNFilter::Fraction` variant and a `test_filter_too_many_n_fraction` unit test. The bug appears to be in the CLI dispatch — the value is parsed as integer/absolute and the `Fraction` branch is never reached.

The fix is likely a one-shot rework of the `--max_n` clap definition: parse as `f32`, then dispatch based on whether the value is `< 1.0` (fraction) or `>= 1.0` (absolute integer).

**Severity rationale**

A user migrating an existing `--max_n 0.5` invocation from Perl to Rust would silently get *more* reads through the filter than they expected, with no error. Could quietly affect downstream analyses.

**Source**

Found during a Phase 1B differential parity hunt (39 fixture-based tests). Full context, reproducers for additional findings, and the harness used: <https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md#f1--max_n-05-fractional-max-n>

Happy to send a PR if helpful.

---

## Issue 2 — `[BUG | HIGH]` `--clip_r1` / `--clip_r2` / `--three_prime_clip_r{1,2}` (lowercase) rejected by Rust

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/242

**Body:**

The Perl `trim_galore` accepts both `--clip_r1` (lowercase, the documented Perl spelling — see the Perl source at `master:trim_galore` line ~1100) and `--clip_R1` (uppercase). The Rust port accepts only `--clip_R1` (uppercase) and rejects `--clip_r1` with a clap parse error (exit code 2, `error: unexpected argument '--clip_r1' found`).

Affected flags (all four):

- `--clip_r1` (lowercase) → must also accept this
- `--clip_r2` (lowercase) → must also accept this
- `--three_prime_clip_r1` (lowercase) → must also accept this
- `--three_prime_clip_r2` (lowercase) → must also accept this

**Reproducer**

```bash
# Perl: works
trim_galore_perl --clip_r1 5 -o /tmp/perl_out test_files/illumina_10K.fastq.gz
echo $?   # 0

# Rust: fails with clap parse error
./target/release/trim_galore --clip_r1 5 -o /tmp/rust_out test_files/illumina_10K.fastq.gz
echo $?   # 2
# error: unexpected argument '--clip_r1' found
```

**Suggested fix**

In `src/cli.rs`, add lowercase aliases to the affected fields:

```rust
#[arg(long = "clip_R1", alias = "clip_r1")]
clip_r1: Option<usize>,

#[arg(long = "clip_R2", alias = "clip_r2")]
clip_r2: Option<usize>,

#[arg(long = "three_prime_clip_R1", alias = "three_prime_clip_r1")]
three_prime_clip_r1: Option<usize>,

#[arg(long = "three_prime_clip_R2", alias = "three_prime_clip_r2")]
three_prime_clip_r2: Option<usize>,
```

(field names are illustrative; match whatever the existing `Cli` struct uses.)

**Note on the existing Perl-flag-rewriter hook**

The CHANGELOG for beta.3 mentions a pre-parser hook that rewrites `-r1`/`-r2`/`-a2` short-form flags to their long-form equivalents for back-compat. That hook covers the *short*-flag forms only — the long-form lowercase spellings still hit clap's case-sensitive matching directly.

**Severity rationale**

Every Perl-era pipeline using the *documented* Perl spelling `--clip_r1` currently breaks under Rust. This is a backwards-compatibility regression with a trivial fix.

**Source**

Found during Phase 1B differential parity hunt. Full context: <https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md#f2--clip_r1-lowercase-rejected>

Happy to send a PR if helpful — the change is ~4 lines.

---

## Issue 3 — `[BUG | HIGH]` `--basename foo` with `--paired` produces `foo_R1_val_1.fq.gz` instead of `foo_val_1.fq.gz`

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/244

**Body:**

In paired-end mode, `--basename foo` is documented to produce `foo_val_1.fq.gz` and `foo_val_2.fq.gz`. The Rust port instead produces `foo_R1_val_1.fq.gz` and `foo_R2_val_2.fq.gz` — an extra `_R1`/`_R2` segment is interpolated between the basename and the `_val_N` suffix. The single-end `--basename` path (which produces `foo_trimmed.fq.gz`) works correctly; only the paired-end branch differs.

The byte content of the trimmed reads matches Perl's output — only the filename differs.

**Reproducer**

```bash
# Perl 0.6.11 — produces foo_val_1.fq.gz, foo_val_2.fq.gz
trim_galore_perl --paired --basename foo -o /tmp/perl_out \
    test_files/BS-seq_10K_R1.fastq.gz test_files/BS-seq_10K_R2.fastq.gz
ls /tmp/perl_out/*.fq.gz
# /tmp/perl_out/foo_val_1.fq.gz
# /tmp/perl_out/foo_val_2.fq.gz

# Rust — produces foo_R1_val_1.fq.gz, foo_R2_val_2.fq.gz
./target/release/trim_galore --paired --basename foo -o /tmp/rust_out \
    test_files/BS-seq_10K_R1.fastq.gz test_files/BS-seq_10K_R2.fastq.gz
ls /tmp/rust_out/*.fq.gz
# /tmp/rust_out/foo_R1_val_1.fq.gz
# /tmp/rust_out/foo_R2_val_2.fq.gz

# Content md5 verification — same
diff <(gzip -dc /tmp/perl_out/foo_val_1.fq.gz) <(gzip -dc /tmp/rust_out/foo_R1_val_1.fq.gz)
# (no diff — content identical, only filename differs)
```

**Hypothesis on fix location**

`src/io.rs` filename-construction for the paired-end + `--basename` path appears to interpolate the `_R1`/`_R2` token from the input filename rather than treating `--basename` as a complete replacement. The Perl logic is `${basename}_val_${N}.fq.gz` directly.

**Pipeline impact**

Snakemake / Nextflow / nf-core pipelines that construct expected output paths as `${basename}_val_1.fq.gz` (the documented Perl convention) silently fail when migrating to Rust because the file simply doesn't exist at that path.

**Source**

Found during Phase 1C differential parity hunt: <https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md#f3--basename-paired-end-filename-pattern>

---

## Issue 4 — `[DISCUSSION]` Three behavioural divergences from Perl 0.6.11 needing classification

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/245

**Body:**

This is a discussion-style issue — three places where Rust v2.1.0-beta.5 produces different output from Perl 0.6.11 and we'd like a project-lead decision on whether the divergence is intentional (document it) or unintentional (fix it). Each is independent; please reply per item with the chosen classification.

### A — Output gzip-compression follows input extension in Perl, always gzipped in Rust

| | |
|---|---|
| Perl behaviour | `.fastq` input → `.fq` (plain) output. `.fastq.gz` input → `.fq.gz` (gzipped) output |
| Rust behaviour | always `.fq.gz` regardless of input extension |
| Why this matters | Pipelines globbing `*.fq.gz` silently miss outputs from plain-`.fastq` inputs under Perl, and migrating to Rust suddenly produces extra `.gz` files for those same inputs |
| Two readings | (1) **Rust is correct** — always gzip for consistency, document as v2.x improvement. (2) **Match Perl** — read input file extension and mirror compression in output |

The Phase 1+2 validation matrix never caught this because every fixture under `test_files/` is `.fastq.gz`. It surfaced when a property-based parity test happened to generate plain-`.fastq` input.

Reproducer:

```bash
# Plain .fastq input
echo -e '@read0\nACGT\n+\nIIII' > /tmp/in.fastq
trim_galore_perl -o /tmp/perl /tmp/in.fastq && ls /tmp/perl/  # in_trimmed.fq (no .gz)
./target/release/trim_galore -o /tmp/rust /tmp/in.fastq && ls /tmp/rust/  # in_trimmed.fq.gz
```

### B — `-a 'A{15}'` (Perl-style brace shorthand) produces different trimmed output

| | |
|---|---|
| Perl md5 of `PolyA_trimmed.fq.gz` content | `f7041578f562544323be055c1544b3e8` |
| Rust md5 | `bec0aacbc27939c7db1b1f24e6e07cd8` |
| Both produce | non-empty output |

CHANGELOG beta.3 introduces "Perl-style `A{N}` single-base expansion" so v2.x explicitly claims parity here — but the trimmed outputs differ. Possibilities: (1) one impl treats `{15}` literally as a 4-character adapter, (2) both expand to `AAAAA…` but use different alignment / overlap parameters, (3) shell-quoting differs in how the brace token reaches each binary. Investigation (e.g., `-a 'A{15}'` vs `-a "A{15}"` vs `-a AAAAAAAAAAAAAAA` literal) would disambiguate.

Reproducer: `trim_galore [perl|rust] -a 'A{15}' -o /tmp/out test_files/PolyA.fastq.gz`

### C — `--paired --retain_unpaired --length 80` produces different `_unpaired_*.fq.gz` content

| | |
|---|---|
| Perl `_val_1.fq.gz` md5 | empty (`d41d8c…`) |
| Perl `_val_2.fq.gz` md5 | empty (`d41d8c…`) |
| Perl `_unpaired_1.fq.gz` md5 | `12fde5c…` (= **full input md5** of `BS-seq_10K_R1.fastq.gz`) |
| Perl `_unpaired_2.fq.gz` md5 | `f71424d…` |
| Rust `_val_1.fq.gz` md5 | empty (`d41d8c…`) |
| Rust `_val_2.fq.gz` md5 | empty (`d41d8c…`) |
| Rust `_unpaired_1.fq.gz` md5 | empty (`d41d8c…`) |
| Rust `_unpaired_2.fq.gz` md5 | empty (`d41d8c…`) |

With `--length 80` filtering all reads on both sides, no reads pass the length filter. **Two valid readings:**

- **Reading A — Perl is correct, Rust is buggy.** The intent of `--retain_unpaired` is to capture reads where one mate fails but the other passes. With both mates failing, Perl is putting the entire R1 into `_unpaired_1` (md5 matches the full input), suggesting Perl bypasses the length filter for unpaired-routing decisions.
- **Reading B — Rust is correct, Perl is buggy.** With everything failing length 80, no reads should be retained anywhere. Rust's behaviour is logically consistent; Perl is dumping all R1 reads into unpaired regardless of whether they pass the length filter.

A semi-trivial test to disambiguate: a fixture where some reads are >80 bp on R1 but all >80 bp on R2, then check whether the >80bp R1 reads (whose mate passes) are correctly routed to unpaired.

Reproducer: `trim_galore [perl|rust] --paired --retain_unpaired --length 80 -o /tmp/out test_files/BS-seq_10K_R1.fastq.gz test_files/BS-seq_10K_R2.fastq.gz`

**Source for all three**

<https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md>

---

## Issue 5 — `[TRACKING | TESTS]` Test coverage gaps: zero unit tests in `parallel.rs` / `demux.rs` + 6 bug-derived regression candidates

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/246

**Body:**

A cross-cutting audit of v2.x test coverage surfaces two related gaps that this issue tracks for incremental closure. Filing as one umbrella because each item is "add unit tests for X" — separate PRs, but a single issue closes incrementally.

### Module-level test gaps

`cargo test` has 165 unit tests across 11 source files, but two notable modules have zero:

- [ ] **`src/parallel.rs` (0 tests).** The `--cores N` worker pool is the highest-risk concurrency surface. Currently exercised only end-to-end via the CI `validation` job's md5 oracle. Suggested coverage: chunk-ordering preservation, gzip-member concatenation correctness, error propagation when a worker panics, and per-thread stat aggregation parity (see §5.2 below).
- [ ] **`src/demux.rs` (0 tests).** Pure barcode-matching logic; only validated end-to-end via the Perl-md5 comparison. Lowest-effort gap to fill.

### Bug-derived regression test candidates

Cross-referencing every bug fix in `CHANGELOG.md` (v2.0.0+) against existing `#[test]` functions surfaces six fixes that shipped without an accompanying regression test:

- [ ] **§5.1 Multi-member gzip FASTQ decoding** (commit `9dcf519`). The `--cores N` parallel mode produces multi-member gzip output (each worker writes its own gzip member, concatenated — RFC 1952). The reader must round-trip such files. A regression here silently truncates parallel-mode output read back as a follow-on input. Test target: `src/fastq.rs`.
- [ ] **§5.2 Parallel/serial stat-tracking parity** (commits `82d1e34`, `3996fc5`). The original bug was `total_bp_after_trim` and `rrbs_r2_clipped_5prime` drifting between the parallel and serial paths. Suggested test: equivalence assertion on `TrimStats` between `--cores 1` and `--cores 4` on the same fixture. Test target: `src/parallel.rs` (also closes the parallel.rs gap above).
- [ ] **§5.3 Adapter auto-detection 1M-read scan limit** (commit `9129650`). The constant `MAX_SCAN_READS = 1_000_000` exists in `src/adapter.rs:85`, but no test asserts the boundary behaviour. Test target: `src/adapter.rs`.
- [ ] **§5.4 PE param-summary line "-end" suffix typo prevention** (beta.3). The original bug was `removed-end:` instead of `removed:`. Trivial test asserting absence of `"removed-end"` in the rendered PE param-summary text. Locks down a class of typo regression that breaks MultiQC parsers. Test target: `src/report.rs`.
- [ ] **§5.5 Demux CRLF samplesheet handling** (v0.6.11 inherited). Windows-authored barcode samplesheets with `\r\n` line endings — Perl strips `\r`; Rust currently has 0 unit tests so behaviour is unverified. Test target: `src/demux.rs`.
- [ ] **§5.6 Demux NoCode routing for short reads** (v0.6.11 inherited). `src/demux.rs:179-184` has explicit "Read too short for barcode — goes to NoCode" handling. No unit test asserts it. Test target: `src/demux.rs`.

### Already landed (FYI, not asks)

- The `tests/parity_proptest.rs` and `tests/parity_fuzz.rs` integration tests on a fork branch (`an-altosian/TrimGalore`) constitute a working start on differential property-based testing — 463 differential runs across 11 flag paths so far. These could be upstreamed as a separate PR if useful.
- `proptest = "1"` and `rand = "0.8"` are already declared as dev-deps on that fork branch.

### Full background

Cross-cutting audit doc: <https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_REVIEW_ci-cd-audit.md> (Part 4 #1/#2, Part 5 §5.1–§5.6)

---

## Issue 6 — `[TRACKING | CI]` CI / test-infrastructure improvements from cross-cutting audit

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/247

**Body:**

A cross-cutting audit of `.github/workflows/ci.yml` surfaces a set of CI / test-infrastructure improvements, prioritized for value-per-effort. Filing as one umbrella because each item is independent CI work; close incrementally.

### Highest-value items

- [ ] **`failure()` artifact upload on the `validation` job.** When a step fails, the `/tmp/op*` outputs that triggered the md5 mismatch are lost. Adding an `if: failure()` step that uploads `/tmp/op*` and `/tmp/tg*` directories turns a 30-minute "what did the binary actually produce?" investigation into a one-click download. Trivial change, big debug-time win.
- [ ] **macOS runner in `rust-tests`.** Cross-platform regressions currently surface only at release-build time on `macos-latest`. Adding `matrix.os: [ubuntu-latest, macos-latest]` to the existing `rust-tests` job costs ~2× wall-clock but covers the macOS Apple Silicon target before it's tagged for release.
- [ ] **`cargo test --release` step.** The `Cargo.toml` has `lto = true, codegen-units = 1` for release; the existing `rust-tests` step uses default debug. Release builds can behave subtly differently (LTO interaction, optimization-dependent code paths) — a `--release` step catches LTO-only bugs.
- [ ] **Coverage reporting** (`cargo-llvm-cov` or `cargo-tarpaulin`). With 165 unit tests, line/branch coverage measurement would identify where new tests have the highest marginal value (almost certainly `parallel.rs`, `demux.rs` per Issue 5). One new job; useful HTML artifact.

### Oracle / fixture pinning

- [ ] **Use `git show origin/master:trim_galore` instead of curl from upstream URL.** The `validation` job currently does `curl -fsSL https://raw.githubusercontent.com/FelixKrueger/TrimGalore/0.6.11/trim_galore`. Since the Perl source lives at `master:trim_galore` in this same repo, replacing the curl with a `git show` (or a `git worktree add`) gives byte-identical content with zero external network dependency. Original audit recommendation was "SHA-pin the URL" — using local `master` is strictly better.
- [ ] **Pin Cutadapt's bioconda revision.** `cutadapt=5.2` in CI is exact-version-pinned but the bioconda revision is not. A new `5.2-1` build could subtly change reference output and silently shift the validation md5 baseline. Pin to `cutadapt=5.2=*_0` (or current revision) and bump deliberately.

### Developer experience

- [ ] **`justfile` (or `Makefile`) for local CI parity.** Reproducing the `validation` job locally currently requires reading 17 YAML steps. A `just validate` target encoding the same logic lets contributors verify before pushing. Pairs well with extracting the bash bodies from `ci.yml` into a `scripts/validate.sh` referenced by both CI and `just`.
- [ ] **`cargo nextest` adoption.** Process isolation matters because at least one test in `src/adapter.rs` writes to a shared `std::env::temp_dir()` path. nextest gives one-process-per-test, per-test timeouts, dramatically better failure output. One-line CI swap.

### Already landed (FYI, not asks)

- Fork branch `an-altosian/TrimGalore@optimus_prime` has a working differential parity-hunt harness (`tests/parity_proptest.rs`, `tests/parity_fuzz.rs`) that auto-skips when Perl/Cutadapt aren't on `PATH` — could be upstreamed as a separate PR. The `proptest = "1"` dev-dep is already declared.

### Full background

Cross-cutting audit doc: <https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_REVIEW_ci-cd-audit.md> (Parts 4 #3/#4/#5/#23/#24/#26/#27/#28, Part 6 §C/§D)

---

## Filing order and procedure

1. Issue 2 first (clip_r1 case-sensitivity) — cleanest, smallest, possibly the trivial-fix-PR offer lands well.
2. Issue 1 (`--max_n 0.5`) — second HIGH bug.
3. Issue 3 (`--basename` PE) — third HIGH bug.
4. Issue 4 (discussion / triage) — needs maintainer input, file last among bug-related.
5. Issue 5 (test coverage tracking).
6. Issue 6 (CI infrastructure tracking).

After each `gh issue create` completes, append the resulting issue number/URL to the corresponding `**Filed at:**` line above and commit the update.

Filing command template:

```bash
gh issue create --repo FelixKrueger/TrimGalore \
    --title "[BUG | HIGH] --max_n 0.5 (fractional) silently ignored — outputs as if no filtering applied" \
    --body-file /tmp/issue1_body.md
```

---

## Issue 7 — `[PERF]` Profiling report: gzip dominates 60.7% of CPU + 3 quick-win optimizations (~25–45% wall improvement)

**Filed at:** https://github.com/FelixKrueger/TrimGalore/issues/248

**Body:**

## Summary

Profiling and benchmarking on the `optimus_prime` fork (commit [`a840fc2`](https://github.com/an-altosian/TrimGalore/commit/a840fc2)) confirms **gzip compression accounts for 60.7% of CPU time** at `--cores 1` and 59.1% at `--cores 8`. Three quick-win optimizations targeting the gzip-dominated path could plausibly deliver **25–45% wall-clock improvement** with low total LOC change.

This issue presents the data and proposes concrete changes for triage. Full methodology and per-function review: [`docs/plans/2026-04-28_AUDIT_performance.md`](https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_AUDIT_performance.md).

## Methodology

- **Wall-clock benchmark**: `hyperfine 1.20.0`, 10 runs + 1 warmup discarded per setting.
- **Sample-based profiling**: `pprof-rs` (POSIX SIGPROF, no kernel perf permissions needed). 10 runs at `--cores 1` and `--cores 8`, folded-stack outputs concatenated, then categorically tallied. Total merged sample budget: 3,950 (cores=1) and 814 (cores=8).
- **Fixture**: 1M-read synthetic input = `test_files/smallRNA_100K.fastq.gz` × 10 (multi-member gzip — RFC 1952; the reader handles this).
- **Build**: `cargo build --release` (the project's standard release profile: `lto = true`, `codegen-units = 1`, opt-level 3).
- **Reproducibility**: full harness committed at `examples/profile_smallrna.rs`; raw artifacts at [`docs/plans/perf_data/`](https://github.com/an-altosian/TrimGalore/tree/optimus_prime/docs/plans/perf_data).

## Wall-clock scaling (1M-read fixture, 10 runs each)

| Cores | Mean (s) | StdDev | Range (min … max) | Speedup vs cores=1 | User time (s) |
|---:|---:|---:|---:|---:|---:|
| 1 | 3.044 | ±0.009 (0.3%) | 3.031 … 3.058 | 1.00× | 3.029 |
| 2 | 1.971 | ±0.016 (0.8%) | 1.950 … 1.997 | 1.55× | 3.898 |
| 4 | 1.171 | ±0.011 (0.9%) | 1.158 … 1.195 | 2.60× | 4.018 |
| **8** | **0.954** | ±0.031 (3.2%) | 0.927 … 1.017 | **3.19×** | 4.677 |
| 16 | 0.974 | ±0.028 (2.9%) | 0.936 … 1.021 | 3.13× | 5.803 |
| 32 | 0.984 | ±0.043 (4.4%) | 0.889 … 1.029 | 3.09× | 5.866 |

**Two findings worth surfacing**:

1. **The README's "near-linear speedup up to ~16 cores" overstates** the actual behaviour. cores=8/16/32 confidence intervals overlap heavily ([0.892, 1.016] vs [0.918, 1.030] vs [0.898, 1.070]). The plateau is real; the precise knee location is not pinpointable. cores=8 is empirically the fastest mean but not statistically distinct from 16 or 32.

2. **User time grows monotonically with cores** (3.0 → 4.0 → 4.7 → 5.8 → 5.9 from 1→4→8→16→32 cores). At 16+ cores user time roughly doubles vs cores=1 without wall-clock improvement — overhead growth is real but is not localised to a single subsystem (see breakdown below).

md5 byte-identity holds across `--cores` 1–32 (the multi-core determinism property from CHANGELOG, verified empirically across all 60 runs).

## Sample-based CPU breakdown (10-run merged)

| Component | cores=1 (3,950 samples) | cores=8 (814 samples) |
|---|---:|---:|
| `zlib_rs` (gzip compression) | **51.9%** | **50.6%** |
| `trim_galore::trimmer::*` (incl. inlined alignment) | **35.1%** | **35.6%** |
| `crc32fast` (gzip CRC) | **8.8%** | **8.5%** |
| `trim_galore::fastq` (I/O, String allocs) | 3.2% | 3.2% |
| Other | 1.0% | 2.1% |

**Combined gzip work: 60.7% at cores=1, 59.1% at cores=8** — confirms the CHANGELOG estimate "the dominant cost (gzip compression, ~60% of runtime)" to high precision. The proportional breakdown is essentially identical at cores=1 and cores=8, indicating the parallel pipeline scales each subsystem evenly without one becoming a serial bottleneck.

## Top hot functions (cores=1, 3,950-sample merged)

| Inclusive samples | Function |
|---:|---|
| 2,435 (61.6%) | `trim_galore::fastq::FastqRecord::write_to` |
| 1,968 (49.8%) | `zlib_rs::deflate::algorithm::medium::deflate_medium` |
| 1,377 (34.9%) | `trim_galore::trimmer::trim_read` |
| 1,376 (34.8%) | `zlib_rs::deflate::longest_match::longest_match` |
| 349 (8.8%) | `crc32fast::baseline::update_fast_16` |

`FastqRecord::write_to` is the dominant single function — it sits on the gzip critical path: `write_to → write_fmt → write_all → flate2 → deflate_medium → longest_match`. It currently calls `writeln!` 4 times per record (header, sequence, `+`, quality), each writeln passing a small chunk to deflate.

`zlib_rs::deflate::algorithm::medium::deflate_medium` is the deflate algorithm used at compression level 6 (the project's default). Levels 1–3 use `quick`, level 4 uses `quick`, levels 5–9 use `medium`. Lowering from 6 to 4 bypasses `deflate_medium` entirely.

## Proposed quick wins (sample-grounded)

| # | Change | File | Effort | Expected wall improvement |
|---:|---|---|---|---:|
| 1 | **Lower default gzip compression level 6 → 4** (or expose `--fast-gz`) | `fastq.rs:454`, `parallel.rs:249,250,252,257,563` | ~5 LOC | **20–35%** |
| 2 | **Single buffered write per record in `FastqRecord::write_to`** (build into a local `Vec<u8>`, single `write_all`) | `fastq.rs:42-48` | ~15 LOC | **5–10%** |
| 3 | **Increase per-batch size 4096 → 16384 records** (let deflate see bigger chunks) | `parallel.rs:32`, `fastq.rs:126` | ~2 LOC | **3–8%** |

Composite estimate for items 1+2+3: **25–45% wall-clock improvement** at `--cores 8` on the 1M-read fixture, all targeting gzip dominance directly. None require algorithmic changes.

A larger discrete optimisation worth considering separately:

| # | Change | Effort | Expected gain |
|---:|---|---|---:|
| 4 | **Myers' bit-parallel edit distance** for adapters ≤64 bp (replace the semi-global DP in `alignment.rs::find_3prime_adapter`) | ~400 LOC + tests | **10–20%** of total wall (cuts the 36% `trim_read` budget by ~30%) |

This is a higher-risk change because the existing DP is the parity oracle for 19 flag paths in `tests/parity_proptest.rs`. Any replacement must preserve byte-identity (verifiable by running the harness — ~12 minutes).

## Points where the original code-review-only audit was wrong

For the methodology record (these are surfaced to help calibrate future code-review-only estimates):

- **`String → Vec<u8>` for `FastqRecord::seq`/`qual`**: code-review predicted 5–15% wall improvement. Sample data: fastq module is 3.2% of total. Real impact: 1–3%. Worth doing for ergonomics (eliminates `.as_bytes()` boilerplate) but not for perf.
- **Flat DP matrix in `find_3prime_adapter`**: predicted 5–10% wall improvement from eliminating per-call `Vec<Vec<usize>>` allocation. Sample data: invisible at this resolution. Real impact: <1%.
- **Threaded-reader `Vec<Option<FastqRecord>>` instead of `mem::replace` with empty Strings**: predicted 2–5%. Sample data: doesn't appear in top frames. Real impact: <1%.

The lesson: **code-review estimates of allocation cost without sample data systematically overestimate**. Modern allocators are very fast for sub-1KB allocations (tens of nanoseconds); 1M of them adds up to ~50 ms, drowned in 4.3 seconds of gzip work.

## Verification approach for any optimisation

Before merging any item from the proposed list:

1. **Run `tests/parity_proptest.rs`** (~12 minutes) — verify byte-identity preserved across 20 flag paths against Perl 0.6.11.
2. **Run the wall-clock benchmark** (`hyperfine` invocation in [`docs/plans/perf_data/hyperfine_scaling_10runs.md`](https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/perf_data/hyperfine_scaling_10runs.md)) at `--cores 1` and `--cores 8`, compare against the baseline numbers in the audit doc.
3. **Run `examples/profile_smallrna.rs`** at the new compression level and confirm zlib_rs's share of CPU drops.
4. **Run the CI `validation` job locally** — md5-compare against Perl 0.6.11 across all 5 protected paths.

For lowering the default compression level (item 1): note that **changing the default level breaks the validation matrix's md5 oracle** (gzip output bytes will differ even though decompressed content matches). Two ways to handle:

- **Option A**: keep level 6 as default, add `--fast-gz` (or `--compression-level N`) flag for users who want speed. Validation matrix unchanged.
- **Option B**: change default to level 4 globally, update the validation matrix's expected md5s, document the change in CHANGELOG. Faster by default but breaks back-compat for anyone diffing v2.x output bytes against earlier versions.

Option A is back-compat-safer; Option B has bigger user-facing impact. Either is fine — needs a project-lead decision.

## What deliberately should NOT change (perf-wise)

| Component | Why |
|---|---|
| `quality.rs` | Already optimal. Single backward pass, no allocation, branch-predictable. Sample data: 0 leaf-level samples in the quality module |
| `crc32fast` | Already SIMD (samples show `update_fast_16` and `pclmulqdq::calculate`). External crate, not our code |
| `MultiGzDecoder` (input) | Only 8 samples in 3,950 = 0.2%; not the bottleneck |
| Output filename construction (`io.rs`) | Two correctness bugs already filed (#244 F3, #245 P3-F2); perf is fine |

## Cross-references

- Sister tracking issue #246 (test coverage) — verification of any perf optimisation depends on the proptest harness landing in CI; happy to PR that separately if useful.
- Sister tracking issue #247 (CI infrastructure) — `hyperfine` runs would let CI catch perf regressions; not currently wired up.
- Audit doc (full per-function review): [`docs/plans/2026-04-28_AUDIT_performance.md`](https://github.com/an-altosian/TrimGalore/blob/optimus_prime/docs/plans/2026-04-28_AUDIT_performance.md)
- Profiling harness: [`examples/profile_smallrna.rs`](https://github.com/an-altosian/TrimGalore/blob/optimus_prime/examples/profile_smallrna.rs)
- Raw flamegraphs + folded stacks: [`docs/plans/perf_data/`](https://github.com/an-altosian/TrimGalore/tree/optimus_prime/docs/plans/perf_data)

Happy to send a PR for any of the top-3 quick wins. The trivial #1 (gzip level toggle) and #3 (batch size) are good first cuts because they're low-risk and the parity harness verifies byte-identity preservation in ~12 minutes.


---

## Comment on Issue 7 (#248) — funnel finding

**Posted at:** https://github.com/FelixKrueger/TrimGalore/issues/248#issuecomment-4339553695

**Why posted:** Measuring `gzip=false` to put a number on the upper bound of compression-level tuning revealed a counterintuitive result — at cores=8, disabling gzip is 1.5× slower, not faster. This exposed the main-thread `mpsc → BTreeMap → write_all` funnel as a hidden serial bottleneck. Compression was doing double duty as a bandwidth-shaper. The comment proposes a 5th quick win (per-worker output files) and notes that lowering the gzip level alone may not fully realise the predicted gain at high core counts without first addressing the funnel.
