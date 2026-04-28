# 2026-04-28 — Draft upstream issues

| | |
|---|---|
| **Status** | DRAFT — review before filing |
| **Target repo** | `FelixKrueger/TrimGalore` |
| **Filer** | `an-altosian` (via `gh issue create`) |
| **Source** | Parity-hunt findings + CI/test audit on `optimus_prime` (commit `41926c7`) |
| **Total** | 6 issues — 3 bug, 1 discussion, 2 tracking |

Each section below is the literal body to file. Titles include severity tags as agreed.
After filing, append the resulting issue URL beside each title for traceability.

---

## Issue 1 — `[BUG | HIGH]` `--max_n 0.5` (fractional) silently ignored — outputs as if no filtering applied

**Filed at:** _(URL after filing)_

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

**Filed at:** _(URL after filing)_

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

**Filed at:** _(URL after filing)_

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

**Filed at:** _(URL after filing)_

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

**Filed at:** _(URL after filing)_

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

**Filed at:** _(URL after filing)_

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
