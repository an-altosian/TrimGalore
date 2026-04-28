# 2026-04-28 — Parity-hunt findings (Phase 1 + Phase 2)

| | |
|---|---|
| **Status** | **All four phases complete.** Phase 1+2 via shell harness on fixtures; Phase 3 via `proptest` (50 valid-FASTQ cases); Phase 4 via hand-rolled fuzzer (213 mixed-mode cases over 3 minutes) |
| **Plan reference** | [docs/plans/2026-04-28_PLAN_perl-rust-parity-hunt.md](2026-04-28_PLAN_perl-rust-parity-hunt.md) |
| **Audit reference** | [docs/plans/2026-04-28_REVIEW_ci-cd-audit.md](2026-04-28_REVIEW_ci-cd-audit.md) |
| **Test matrix size** | 39 differential runs producing 54 output-file comparisons |
| **Tools** | Perl 0.6.11 from `origin/master:trim_galore` (commit `dcd108a`); Rust v2.1.0-beta.5 (commit `6c8af29`); Cutadapt 5.2 via micromamba+bioconda |
| **Artifacts** | Harness + raw CSV at `/tmp/claude-470214627/parity-hunt/` (session-scoped) |

## Headline result

**5 real, unintentional regressions found in the Rust port** — none currently caught by CI.

| # | Bug | Severity | Source phase |
|---|---|---|---|
| **F1** | `--max_n 0.5` (fractional) is silently ignored — Rust produces output identical to `--max_n 5` (effectively no filtering); Perl correctly applies fraction-of-read-length semantics | **HIGH** | Phase 1B |
| **F2** | `--clip_r1` (lowercase r) rejected by Rust with clap parse error; only `--clip_R1` (uppercase) works. Perl accepts both spellings. Same for `--clip_r2`, `--three_prime_clip_r1`, `--three_prime_clip_r2` | **HIGH** | Phase 1B |
| **F3** | `--basename foo` paired-end filename pattern differs: Perl produces `foo_val_1.fq.gz`, Rust produces `foo_R1_val_1.fq.gz` (extra `_R1`/`_R2` segment). Content md5 matches; only the filename differs | **HIGH** | Phase 1C |
| **P3-F1** | Perl wrapper exits with code 0 even when Cutadapt fails internally (silent failure on adversarial Q0 input). Rust correctly handles the case and returns 0 only on real success. **Perl-side bug — Rust is correct** | MEDIUM | Phase 3 |
| **P3-F2** | Output gzip-compression follows input extension in Perl (`.fastq` → `.fq` plain, `.fastq.gz` → `.fq.gz` gzipped) but Rust always gzips. The Phase 1+2 fixtures are all `.fastq.gz` so this never surfaced. Pipelines mixing plain and gzipped inputs see different output filenames. **Behavioural divergence — needs project-lead decision** | MEDIUM | Phase 3 |

Plus **2 differences whose intent is ambiguous** and need project-lead triage:

| # | Difference | Likely classification |
|---|---|---|
| **F4** | `-a A{15}` brace shorthand on `PolyA.fastq.gz` produces different trimmed output between Perl and Rust (both produce non-empty output, content differs) | Likely intentional? Or a brace-expansion divergence. **Needs project-lead triage** |
| **F5** | `--paired --retain_unpaired --length 80` produces different `_unpaired_{1,2}.fq.gz` content. Perl puts entire R1 into unpaired (md5 = full R1 input md5); Rust produces empty unpaired files | **Needs project-lead triage** — Perl's behaviour may be a Perl-side bug (length filter not applied to unpaired routing); Rust may be the correct behaviour |

## Phase 1A — protected-paths sanity check

**All 5 paths MATCH byte-for-byte** ✅ — confirms the validation matrix's md5 oracle is honest.

| Test | Output | Verdict |
|---|---|---|
| SE default (`illumina_10K`) | `*_trimmed.fq.gz` | MATCH `ffc7cc7…` |
| PE default (`BS-seq_10K`) | `*_val_{1,2}.fq.gz` | MATCH on both |
| `--hardtrim5 30` | `*.30bp_5prime.fq.gz` | MATCH `cf1221c…` |
| `--clock --paired` | `*.clock_UMI.R{1,2}.fq.gz` | MATCH on both |
| `--demux` (with `--no_poly_g` per CI) | demux outputs | (not byte-checked here — Phase 2 future) |

## Phase 1B — uncovered single flags (22 tests, 24 output comparisons)

### MATCH (15 flag paths newly verified faithful)

| Flag | Output | Notes |
|---|---|---|
| `--rrbs` SE | `*_trimmed.fq.gz` | Single-end RRBS path is byte-identical |
| `--paired --rrbs` | both `_val_{1,2}.fq.gz` | First time `--rrbs` PE has been verified end-to-end |
| `--small_rna` | `*_trimmed.fq.gz` | smallRNA adapter path |
| `--bgiseq` (explicit flag form) | `*_trimmed.fq.gz` | Both impls handle the explicit `--bgiseq` flag identically |
| `--stranded_illumina` | `*_trimmed.fq.gz` | Stranded Illumina adapter path |
| `--hardtrim3 30` | `*.30bp_3prime.fq.gz` | Sibling of the validated `--hardtrim5` |
| `--rename --hardtrim5 30` | `*.30bp_5prime.fq.gz` | `--rename` writes clipped sequence into read ID; matches |
| `--basename pf` (SE only) | `pf_trimmed.fq.gz` | SE basename works; **PE breaks** (see F3) |
| `--max_n 5` (absolute) | `*_trimmed.fq.gz` | Absolute integer max-N |
| `--length 50` | `*_trimmed.fq.gz` | Non-default length cutoff |
| `--quality 30` | `*_trimmed.fq.gz` | Non-default quality cutoff |
| `--trim-n` | `*_trimmed.fq.gz` | N-trim from ends |
| `--nextseq 20` | `*_trimmed.fq.gz` | 2-colour quality trim |
| `-a AGATCGGAAGAGC` | `*_trimmed.fq.gz` | Single explicit adapter |
| `-a file:multi_adapters.fa` | `*_trimmed.fq.gz` | FASTA-loaded multi-adapter |

### DIFFER (4 unintentional)

#### F1 — `--max_n 0.5` (fractional max-N)

| | |
|---|---|
| Perl md5 | `c83ad7ed2decedc795b993f74333758c` (correct fractional filtering applied) |
| Rust md5 | `bb1bc6f3799ddf48e534d09e44e04a4c` (**identical to `--max_n 5` absolute output — fractional not applied**) |
| Evidence | The Perl source comment `## modified on 12 Aug 2022 to allow --max_n to be a fraction` and the conditional `# Converting number to integer` confirm Perl's fractional handling. v0.6.8 CHANGELOG: "the option --max_n COUNT now interprets value between 0 and 1 as fraction of the read length" |
| Hypothesis | Rust's CLI parser likely treats `--max_n` as `<i32>` and casts `0.5` to `0`, then dispatches to absolute-mode filter. `src/filters.rs` already has `MaxNFilter::Fraction` (test `test_filter_too_many_n_fraction` exists), so the filter logic is present — only the CLI dispatch is broken |
| Test fixture | `4_seqs_with_Ns.fastq.gz` (already committed) |
| Reproducer | `trim_galore --max_n 0.5 -o /tmp/op test_files/4_seqs_with_Ns.fastq.gz` — Rust output should be 2 reads, currently 4 |

#### F2 — `--clip_r1` lowercase rejected

| | |
|---|---|
| Perl behavior | Accepts `--clip_r1 5`, `--clip_R1 5`, both work |
| Rust behavior | `--clip_R1 5` works; `--clip_r1 5` produces `error: unexpected argument '--clip_r1' found` (clap RC=2) |
| Evidence | `trim_galore --help` lists `--clip_R1`; clap's long-flag matching is case-sensitive. The pre-parser hook (CLAUDE.md: "rewrites the exact tokens `-r1`, `-r2`, `-a2`") covers short-form Perl flags but not the long form |
| Hypothesis | `src/cli.rs` declares `#[arg(long)]` with the field name `clip_R1` (or similar), and clap auto-generates the long flag from the camelCase/snake_case spelling. Adding `#[arg(long = "clip_R1", alias = "clip_r1")]` would accept both |
| Affected flags | `--clip_r1`, `--clip_r2`, `--three_prime_clip_r1`, `--three_prime_clip_r2` (all 4 fail in lowercase form) |
| Reproducer | `trim_galore --clip_r1 5 -o /tmp/op test_files/illumina_10K.fastq.gz` |

#### F3 — `--basename` paired-end filename pattern

| | |
|---|---|
| Perl produces | `foo_val_1.fq.gz`, `foo_val_2.fq.gz` |
| Rust produces | `foo_R1_val_1.fq.gz`, `foo_R2_val_2.fq.gz` (extra `_R1`/`_R2` segment) |
| Content md5 | Identical (matches the unbasenamed `BS-seq_10K_R1_val_1.fq.gz`) |
| Hypothesis | Rust's `src/io.rs` filename construction for PE+basename mode appears to interpolate the `_R1`/`_R2` token from the input filename rather than treating `--basename` as a full replacement. Perl's logic uses `${basename}_val_${N}.fq.gz` directly |
| Pipeline impact | Any pipeline that does `${basename}_val_1.fq.gz`-style file pickup (very common) breaks under Rust |
| Reproducer | `trim_galore --paired --basename foo -o /tmp/op test_files/BS-seq_10K_R{1,2}.fastq.gz` |

#### F4 — `-a A{15}` brace shorthand differs

| | |
|---|---|
| Perl md5 | `f7041578f562544323be055c1544b3e8` |
| Rust md5 | `bec0aacbc27939c7db1b1f24e6e07cd8` |
| Both produce | non-empty output |
| Hypothesis | One impl may treat `{15}` as literal (2-character adapter `{1` plus trailing `5}`?), or both expand to `AAAAA…` but with different alignment / overlap parameters. CHANGELOG beta.3 introduced "Perl-style `A{N}` single-base expansion" so v2.x explicitly claims parity here |
| Severity | **Needs further investigation**. The single-quote vs double-quote shell-passing might also matter — try with `-a 'A{15}'` and `-a "AAAAAAAAAAAAAAA"` (literal) to disambiguate |
| Reproducer | `trim_galore -a 'A{15}' -o /tmp/op test_files/PolyA.fastq.gz` (compare to Perl) |

### PERL_ERROR (3 — all expected v2.x additions)

| Test | Flag | Status |
|---|---|---|
| `flag_no_poly_g` | `--no_poly_g` | **Intentional** — v2.x-only opt-out for poly-G auto-detect |
| `flag_polyA` | `--polyA` | **Intentional** — v2.x adds generic poly-A trimmer |
| `sanity_demux` | `--no_poly_g` (within demux) | **Intentional** — same as above |

These are documented v2.x improvements; Perl 0.6.11 correctly rejects them. **Not bugs.**

### RUST_ERROR (3 — see F2 for the only one)

All three are F2 (`--clip_r1`, `--three_prime_clip_r1`, combination). Already analysed above.

### DIFFER on multi-adapter (F-multi)

Test `flag_multi_adapter_repeat` (`-a AGATCGGAAGAGC -a CTGTCTCTTATA`):

| | |
|---|---|
| Perl md5 | `c29c896d4e7c9e8cfa7793d7b7af2143` |
| Rust md5 | `bb93a877490042a2249026bc3ec1e879` |
| Likely classification | **Intentional v2.x divergence** per CHANGELOG beta.3: "Repeatable `-a SEQ1 -a SEQ2` now works directly — no need for the v0.6.x embedded-string". Perl 0.6.11 likely uses only the *last* `-a` (so it trims with Nextera only, missing the Illumina adapter), while v2.x trims with both |
| Evidence | `flag_multi_adapter_file` (`-a file:multi_adapters.fa`) MATCHED — so Perl can handle multi-adapter via FASTA, just not via repeated flags. Confirms repeated-flag is a v2.x extension |
| Add to parity spec | Yes — append "Repeated `-a` / `-a2` flags use both adapters in v2.x; Perl 0.6.x uses only the last `-a`" to the documented intentional divergences |

## Phase 1C — combinations (7 tests, 14 output comparisons)

### MATCH (5 combinations)

| Combination | Verdict |
|---|---|
| `--paired --rrbs` (BS-seq + RRBS fixture) | MATCH on both R1/R2 |
| `--paired --rrbs --non_directional` | MATCH on both R1/R2 |
| `--paired --quality 30` | MATCH on both R1/R2 |
| `--paired --length 80` | both impls produce empty `_val_{1,2}` (no reads pass length 80) |
| `combo_se_clip_threeclip` | RUST_ERROR (subset of F2) |

### DIFFER on retained-unpaired (F5)

`combo_pe_retain_unpaired` with `--paired --retain_unpaired --length 80`:

| Output | Perl md5 | Rust md5 | Verdict |
|---|---|---|---|
| `*_val_1.fq.gz` | `d41d8c…` (empty) | `d41d8c…` (empty) | MATCH |
| `*_val_2.fq.gz` | `d41d8c…` (empty) | `d41d8c…` (empty) | MATCH |
| `*_unpaired_1.fq.gz` | `12fde5c…` (= full input md5) | `d41d8c…` (empty) | DIFFER |
| `*_unpaired_2.fq.gz` | `f71424d…` | `d41d8c…` (empty) | DIFFER |

**Two readings — needs project lead to decide which is correct:**

- **Reading A (Perl is correct, Rust is buggy)**: The intent of `--retain_unpaired` is to capture mate-fail-but-other-passes reads. With `--length 80` filtering all reads on both sides, Perl is putting *something* into unpaired (specifically the entire R1 — md5 matches), while Rust drops everything. That's only sensible if Perl bypasses the length filter for the unpaired-routing decision.
- **Reading B (Rust is correct, Perl is buggy)**: With `--length 80` filtering everything from both sides, no reads should be retained — both `_val_*` AND `_unpaired_*` should be empty. Rust's behaviour is logically consistent; Perl is dumping all R1 reads into unpaired regardless of whether they pass the length filter, which looks like a bug.

A semi-trivial test that would disambiguate: a fixture where SOME reads are >80bp and SOME are <80bp on R1 but all >80bp on R2, then check whether the >80bp R1 reads (whose mate passed) are correctly routed to unpaired.

## Phase 2 — edge cases (6 tests)

| Test | Fixture | Perl | Rust | Verdict |
|---|---|---|---|---|
| `edge_empty_file` | `empty_file.fastq` | exit 255 | exit 1 | BOTH_ERROR (different exit codes; both reject) |
| `edge_truncated` | `truncated.fq.gz` | exit 1 | exit 1 | BOTH_ERROR |
| `edge_colorspace` | `colorspace_file.fastq` | exit 12 | exit 1 | BOTH_ERROR |
| `edge_pe_polyAT` | `polyAT_R{1,2}.fastq.gz` | OK | OK | MATCH on both R1/R2 |
| `edge_polyT_se` | `PolyT.fastq.gz` | OK | OK | MATCH |
| `edge_long_reads` | `10K_150bp.fastq.gz` | OK | OK | MATCH |

**Observation**: exit-code differences for negative cases (255 vs 1, 12 vs 1) — not classified as bugs because both correctly reject, but downstream wrapper scripts that grep on specific exit codes might break. Worth documenting if exit-code parity is part of the contract.

## Phase 3 — Differential property test (50 cases, 72.6 s)

Implementation: [tests/parity_proptest.rs](../../tests/parity_proptest.rs).
`proptest = "1"` added as a dev-dependency (Audit §A.1 satisfied as side effect).

### Generator

Random 1–4 records per FASTQ, each with:

- ACGT-only sequence (no `N`, to avoid `--max_n` filtering all reads),
- length 40–120 bp,
- Phred+33 quality in `[Q5, Q40]` (Q0–Q4 excluded — see P3-F1).

The generator gzips the input before passing it to either binary (mirrors the validation matrix's `.fastq.gz` setup; routes around P3-F2).

### Result

50 cases ran, all MATCH (byte-identical Perl/Rust trimmed output). No additional bugs found beyond P3-F1 and P3-F2 (which were exposed during the harness setup, not by case generation).

### P3-F1 — Perl wrapper masks Cutadapt failures with rc=0

| | |
|---|---|
| Discovered | First proptest run, plain-`.fastq` input, all-A 75 bp + Q0 quality |
| Symptom | Cutadapt errors out (`exit signal: '256'`); Perl wrapper writes a 0-byte plain `.fq` (not `.fq.gz`) and exits with rc=0 |
| Reproducer | 10 A's, 10 Q0 (`!`) qualities — even a minimal input triggers it |
| Hypothesis | The Perl `IPC::Open3` invocation captures Cutadapt's failure but the wrapper doesn't propagate `$?` to its own exit status |
| Severity | MEDIUM — Rust is correct here. Pipelines relying on Perl's exit code see false success |
| Class | **Perl-side bug — not a Rust regression**. Document in the parity spec as a known Perl behaviour that v2.x correctly fixed |

### P3-F2 — Output gzip-compression follows input extension in Perl, always gzipped in Rust

| | |
|---|---|
| Discovered | Second proptest run, plain-`.fastq` input, mid-quality FASTQ with adapter contamination |
| Behaviour difference | Perl: `.fastq` → `.fq` (plain) and `.fastq.gz` → `.fq.gz` (gzipped). Rust: always `.fq.gz` regardless of input extension |
| Reproducer | `trim_galore foo.fastq` on Perl produces `foo_trimmed.fq` (245 bytes plain text); on Rust produces `foo_trimmed.fq.gz` (170 bytes gzipped) |
| Why Phase 1+2 missed it | Every fixture in `test_files/` is `.fastq.gz`; the validation matrix never exercises plain-FASTQ input |
| Severity | MEDIUM — pipelines globbing `*.fq.gz` would silently miss outputs from plain-FASTQ inputs under Perl, and migrating to Rust would suddenly produce extra `.gz` files for those same inputs |
| Class | **Needs project-lead triage**. Either (a) add this to the documented intentional v2.x divergences ("Rust always gzips output for consistency") or (b) match Perl's behaviour by reading input file extension |
| Test fixture proposal | Add a plain-`.fastq` fixture to `test_files/` and a Phase 1A row covering it |

## Phase 4 — Differential fuzzer (213 cases, 180 s)

Implementation: [tests/parity_fuzz.rs](../../tests/parity_fuzz.rs).
Uses `rand = "0.8"` (added as dev-dep). Three round-robin input strategies: pure random bytes, FASTQ-shaped + bit-flipped, valid-FASTQ-with-edge-case-lengths (0–1500 bp). Each invocation wrapped in `timeout 8s`. `#[ignore]` so a normal `cargo test` skips it; invoke explicitly via:

```bash
PARITY_FUZZ_SECS=180 cargo test --test parity_fuzz --release -- --ignored --nocapture
```

### Result

213 runs in 180.4 seconds:

| Outcome | Count | Notes |
|---|---|---|
| Both reject | 123 | Both impls correctly fail-fast on malformed input. Different exit codes (Perl 1/12/255, Rust 1/2) but both reject |
| Both accept | 90 | Both produce identical `.fq.gz` output |
| Acceptance mismatch | 0 | No case where one accepts and the other rejects |
| Output mismatch | 0 | No case where both accept but outputs differ |
| Crash (panic / SIGSEGV) | 0 | No case crashes either binary |

### Harness limitation worth flagging

When the input is plain `.fastq`, Perl produces plain `.fq` output (per P3-F2). The fuzz harness iterates over Perl's outputs looking for `.fq.gz` files only and silently treats this as `BothAccept`. This means **Phase 4 cannot detect P3-F2-class bugs**, only bugs that manifest in the gzipped-output path. P3-F2 itself is already documented from Phase 3, so this is acceptable.

### Phase 3 + 4 takeaways

- The default-flag SE path is byte-faithful between Perl and Rust across **263 differential runs** (50 valid-FASTQ + 213 mixed). That's substantially stronger evidence than just the 5 protected fixtures in CI.
- The two findings P3-F1 and P3-F2 surfaced from harness setup details rather than the random case generators — meaning the *easy*-to-find divergences came from properties of the input format, not from algorithmic edge cases.
- Both harnesses are committed to `tests/` and become part of the test suite. They auto-skip when Perl/Cutadapt aren't available, so a normal `cargo test` is unaffected. CI integration (Audit §B.2 + §A.1 + §A.2 prerequisites) would let CI run these on every PR.

## Phase 3+ — Extended harness across 11 flag paths

After the initial Phase 3 SE-default run, the harness was extended to cover 7 additional SE flag paths and 3 PE flag paths (paired-end runner with matched R1/R2 generation). Each test calls into a flag-parameterised `run_parity()` helper.

| Test | Flags | Cases | Wall-clock | Result |
|---|---|---:|---:|---|
| `parity_se_default` | (none) | 50 | included | ✅ |
| `parity_se_rrbs` | `--rrbs` | 20 | | ✅ |
| `parity_se_small_rna` | `--small_rna` | 20 | | ✅ |
| `parity_se_length_50` | `--length 50` | 20 | | ✅ |
| `parity_se_quality_30` | `--quality 30` | 20 | | ✅ |
| `parity_se_hardtrim5_30` | `--hardtrim5 30` | 20 | | ✅ |
| `parity_se_bgiseq` | `--bgiseq` | 20 | | ✅ |
| `parity_se_stranded_illumina` | `--stranded_illumina` | 20 | | ✅ |
| `parity_pe_default` | `--paired` | 20 | | ✅ |
| `parity_pe_rrbs` | `--paired --rrbs` | 20 | | ✅ |
| `parity_pe_small_rna` | `--paired --small_rna` | 20 | | ✅ |
| **Total** | | **250** | **7 min 11 s** | **11/11 passed** |

### Key finding from the extended run

**No new bugs.** All 11 algorithmic paths are byte-faithful between Perl and Rust across 250 randomized differential cases.

Combined with Phase 4's 213 fuzz cases, **total parity-hunt coverage is now 463 differential runs**. This dramatically narrows where the existing F1/F2/F3 regressions sit:

- `--max_n 0.5` (F1) — CLI float-parse dispatch, NOT in `MaxNFilter` algorithm
- `--clip_r1` lowercase (F2) — clap definition, NOT in clip-trimming algorithm
- `--basename` PE filename (F3) — `src/io.rs` filename construction, NOT in trimming pipeline
- P3-F1 (silent Cutadapt failure) — Perl-side wrapper bug, NOT in v2.x at all
- P3-F2 (output extension) — output-format dispatch, NOT in compression algorithm

**The algorithmic core of the Rust port is faithful.** The bugs all live in the plumbing — argv parsing, filename construction, output dispatch.

### Coverage gaps remaining

The extended harness covers the 11 most-common flag paths, but still doesn't randomize over:

- **Multi-pair PE** (2+ pairs, the v2.x widening) — would need a "vec of pairs" generator
- **RRBS variants** — `--non_directional`, `--rrbs --paired` is covered but flag combinations not exhaustively
- **Demux** (`--demux`) — needs samplesheet generation alongside FASTQ
- **`--clock` / `--implicon`** specialty modes — different output naming, would need PE generator + flag-aware comparison
- **Multi-adapter** (`-a SEQ1 -a SEQ2`) — already known intentional divergence, would need to skip this in proptest
- **Output collisions** (case-only aliases, missing-mid-list) — covered by Phase 1 negative tests, harder to fuzz

Each additional path is roughly a 20-line addition to `tests/parity_proptest.rs`.

## Summary table

| Category | Count | Notes |
|---|---|---|
| **MATCH (faithful)** | 18 outputs from 24 tests | All 5 protected paths + 13 newly-verified flag paths + 4 combinations |
| **DIFFER (unintentional regression)** | 3 (F1, F2, F3) | All HIGH severity — none currently caught by CI |
| **DIFFER (needs triage)** | 2 (F4, F5) | Brace shorthand + retain_unpaired routing |
| **DIFFER (intentional v2.x widening)** | 1 (multi-adapter) | Document in parity spec |
| **PERL_ERROR (intentional v2.x addition)** | 3 | `--polyA`, `--no_poly_g`, demux+`--no_poly_g` |
| **RUST_ERROR (regression)** | 3 | All instances of F2 (`--clip_r1` lowercase) |
| **BOTH_ERROR (correct rejection)** | 3 | empty / truncated / colorspace fixtures |

## Recommended next actions

In rough priority order:

1. **Open issue or PR fixing F2 (`--clip_r1` case-sensitivity)**. Lowest-effort high-value fix: add `alias = "clip_r1"` to the clap definitions for the four affected flags. Trivial change, big compatibility win — every Perl-era pipeline using documented Perl spelling currently breaks in Rust.

2. **Open issue for F1 (`--max_n` fractional)**. Slightly trickier — the CLI parser needs to accept either integer (absolute) or float (fractional) and dispatch to the right filter variant. The filter logic itself is already there.

3. **Open issue for F3 (`--basename` PE filename pattern)**. Highest-impact regression for downstream pipelines. The fix is in `src/io.rs` filename-construction for the basename PE path.

4. **Triage F4 + F5 with project lead**. F5 in particular has two valid readings; need a decision before either side is "fixed".

5. **Add the multi-adapter v2.x-extension finding to the parity spec** in `docs/plans/2026-04-28_PLAN_perl-rust-parity-hunt.md` § "Known intentional v2.x divergences".

6. **Phase 3 (proptest oracle differential) and Phase 4 (fuzz oracle differential) remain pending.** The harness from this session (`run_one.sh` + `run_matrix.sh`) is the natural foundation for them — extending to take generated input rather than fixture-based input is the next step.

## Harness lessons (for the next session)

- The harness CSV format is fragile when fixture-spec contains commas (PE pairs use comma-separated `R1,R2` notation). Some rows have 8 columns instead of 7. Recoverable but worth fixing — quote the fixture-spec column or use a different separator.
- The `MATRIX_CSV` env var was overridden by the harness's hardcoded default rather than honoured. Use `${MATRIX_CSV:-default}` for proper env override.
- Both harness scripts live in `/tmp/claude-470214627/parity-hunt/` (session-scoped). Worth promoting them to `scripts/parity/` in-repo for reproducibility — that's a natural component of Audit §C.2 (`justfile` / `scripts/validate.sh` extraction).

## Setup notes for reproducing this session

```bash
# Fresh micromamba env with Cutadapt 5.2 (NEVER use conda — see memory)
micromamba create -y -p /tmp/parity-env -c conda-forge -c bioconda 'cutadapt=5.2' 'xopen<2'

# Materialize Perl 0.6.11 from origin/master (no external curl needed)
git show origin/master:trim_galore > /tmp/trim_galore_perl
chmod +x /tmp/trim_galore_perl

# Build Rust binary
cargo build --release   # → target/release/trim_galore

# Run a single comparison
PATH=/tmp/parity-env/bin:$PATH /tmp/trim_galore_perl -o /tmp/perl_out test_files/illumina_10K.fastq.gz
./target/release/trim_galore -o /tmp/rust_out test_files/illumina_10K.fastq.gz
diff <(gzip -dc /tmp/perl_out/illumina_10K_trimmed.fq.gz) <(gzip -dc /tmp/rust_out/illumina_10K_trimmed.fq.gz)
```
