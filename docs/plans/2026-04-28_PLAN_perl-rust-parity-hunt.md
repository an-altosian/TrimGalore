# 2026-04-28 — PLAN: Perl/Rust parity hunt

| | |
|---|---|
| **Status** | **All four phases COMPLETE on 2026-04-28.** Phase 1+2: 39 fixture-based tests, 3 unintentional regressions (F1–F3, HIGH). Phase 3: 50 `proptest` cases on gzipped synthetic FASTQ, 0 additional bugs after harness setup exposed P3-F1 + P3-F2. Phase 4: 213 fuzzed cases over 180s, 0 divergences (123 both-reject + 90 both-accept). 5 total findings; Rust is correct for P3-F1, project-lead triage needed for P3-F2 |
| **Findings** | [docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md](2026-04-28_FINDINGS_parity-hunt-phase1-2.md) — full results, classifications, and reproducers |
| **Scope** | Find every place v2.x Rust output diverges from Perl Trim Galore 0.6.11; classify each divergence as intentional (v2.x improvement) or unintentional (regression); document or fix accordingly |
| **Related** | [docs/plans/2026-04-28_REVIEW_ci-cd-audit.md](2026-04-28_REVIEW_ci-cd-audit.md) — testing audit (Part 6 §A–§D contain prerequisites listed below) |
| **Author** | Initial scaffold by Claude Code session, dhe@altoslabs.com |

## Purpose

The CI `validation` job protects exactly 5 flag paths with byte-identity md5 comparisons against Perl 0.6.11.
Everything else is potentially divergent and uncaught.
This plan structures a deliberate hunt for unintentional divergences across the rest of the flag and edge-case surface, then triages each finding into "intentional v2.x improvement (document)" or "unintentional regression (fix)".

The hunt is **not** about achieving 100% byte-identity — v2.x has explicit improvements over Perl that should not be reverted.
It's about finding cases where v2.x diverges *without anyone having decided that it should*.

## Prerequisites — from the testing audit

These must be in place before the hunt is productive. Cross-refs are to [the audit doc](2026-04-28_REVIEW_ci-cd-audit.md):

- [x] **Perl source locally available.** Already satisfied — the Perl Trim Galore 0.6.11 source lives on the `master` branch of this same repo (`origin/master` HEAD = `dcd108a` = the `0.6.11` tag, with `trim_galore` at 163 KB as an executable Perl script). Use `git show origin/master:trim_galore` to read the source; use `git worktree add ../tg-perl origin/master` to materialise it for execution. No external fetch needed.
- [ ] **Audit §D.1 — replace the URL fetch in `ci.yml:159`** with `git show origin/master:trim_galore` (or a worktree). The audit doc proposed SHA-pinning the URL; using the local `master` is *strictly better* — same content, zero external dependencies, no GitHub-availability concern. **Load-bearing.**
- [ ] **Audit §D.2 — Pin Cutadapt's bioconda revision.** Cutadapt is part of the Perl reference toolchain; floating revision = floating oracle.
- [ ] **Audit §C.4 — Failure-only artifact upload.** When Perl and Rust differ on a synthetic input, the offending pair of outputs must be downloadable, not lost in CI ephemera.
- [ ] **Audit §C.2 — `justfile` / `scripts/validate.sh`.** Cheap local re-runs; same script CI uses.
- [ ] **Audit §A.1 — `proptest` adoption.** Phase 3 below depends on this.
- [ ] **Audit §A.2 — `cargo-fuzz` adoption.** Phase 4 below depends on this.

> **Note**: CHANGELOG line 238 mentions `legacy/trim_galore` as the future home of the Perl source *on the `optimus_prime` (Rust) branch* once v2.1.0 GA ships. That's a separate concern from "is the source available today" — today, it's on `master` and that's enough.

## Definition of "parity"

Two-part definition: a registry of *known intentional divergences* (these are NOT bugs) and a default of *byte-identical output for everything else*.

### Known intentional v2.x divergences

Maintained in `CHANGELOG.md`'s "Unreleased" / "Documentation" sections; mirrored here for triage convenience. Update both places when adding a new intentional divergence.

#### Trimming-report text (from CHANGELOG #234)

| # | Divergence | Direction |
|--:|---|---|
| 1 | RRBS quality-trim line shape changed from per-read (`Sequences were truncated…: N (P%)`) to cutadapt-style bp-counts (`Quality-trimmed: N bp (P%)`) | v2.x is more consistent across modes; regex tuned to one shape won't match the other |
| 2 | Adapter family-name annotation dropped — v2.x emits bare sequence (`'AGATCGGAAGAGC'`) where v0.6.x emitted family names (Illumina TruSeq, Nextera, smallRNA) | Family is tracked internally but not rendered |
| 3 | "Bases preceding removed adapters" histogram omitted from the `=== Adapter N ===` block | — |
| 4 | Length-distribution `max.err` column uses modern Cutadapt formula `floor(L × error_rate)`; v0.6.x display capped at 1 in many positions | `count` column unchanged byte-for-byte |

#### Behavioural deviations (CHANGELOG, README, CLAUDE.md)

- Per-pair adapter auto-detection (v0.6.x detected once on `$ARGV[0]`).
- Poly-G auto-detection and trimming for 2-colour instruments (v2.x-only; opt-out via `--no_poly_g`).
- Generic poly-A trimmer (new in v2.x).
- Repeatable `-a` / `-a2` multi-adapter syntax (Perl required embedded-string `-a " SEQ -a SEQ"`).
- BGI/DNBSEQ in adapter auto-detection probe set (Perl had explicit `--bgiseq` only).
- JSON trimming report format (v2.x-only).
- MultiQC-compatible Cutadapt section in text reports (v2.x addition).
- `--version` provenance line (v2.x-only).
- `SOURCE_DATE_EPOCH` reproducibility (v2.x-only).

#### Anything else discovered during the hunt

Append to this list; don't silently absorb.

### Unintentional divergences (these ARE bugs)

- Anything that breaks byte-identity on the 5 protected paths in `ci.yml`'s validation matrix (SE, PE, hardtrim5, clock, demux).
- Anything where MultiQC's canonical parser produces different output for v2.x vs v0.6.x (subtle — see #232 / #233 for the class).
- Performance regressions on documented use cases (separate workstream — out of scope here).

## Approach — four phases in increasing cost / increasing thoroughness

| Phase | Technique | Finds | Cost | Maps to audit |
|---|---|---|---|---|
| **1** | **Flag-coverage matrix** — enumerate every `clap` flag, run small differential per uncovered flag | Surface-area gaps. Quick win | Low | Extends Part 5 |
| **2** | **Edge-case differential corpus** — hand-curated fixtures for known edge cases (empty, single-read, all-N, very long, Phred+64, non-ASCII, BGZF) | Known-unknown bugs | Low/Medium | Extends Part 5; reuses CI conda-Perl install |
| **3** | **Differential property testing** — `proptest` generates valid synthetic FASTQ; assert byte-identity modulo registered divergences | Unknown-unknown bugs in covered paths | Medium | Layers on Part 6 §A.1 — same dep, different oracle |
| **4** | **Differential fuzzing** — `cargo-fuzz` with arbitrary bytes; both impls should accept-or-reject identically | Unknown-unknown bugs in malformed-input paths | High | Layers on Part 6 §A.2 — same fuzz infrastructure, different assertion |

## Phase 1 — Flag-coverage matrix

**Template — fill in over future sessions.** Seed with the known-covered five so the gap is visible.

| Flag | Covered today? | Fixture(s) | Phase 1 status | Notes |
|---|---|---|---|---|
| (default SE) | YES — validation matrix | `illumina_10K.fastq.gz` | n/a | Byte-identity guarded |
| `--paired` | YES — validation matrix | `BS-seq_10K_R{1,2}.fastq.gz` | n/a | Byte-identity guarded |
| `--hardtrim5 30` | YES — validation matrix | `illumina_10K.fastq.gz` | n/a | Byte-identity guarded |
| `--clock --paired` | YES — validation matrix | `clock_10K_R{1,2}.fastq.gz` | n/a | Byte-identity guarded |
| `--demux` | YES — validation matrix | `demux_test.fastq.gz` | n/a | Byte-identity guarded |
| `--rrbs` | NO | `SRR24766921_RRBS_R{1,2}.fastq.gz` exists | TODO | Several #232 / #233 nuances; expect intentional divergences |
| `--small_rna` | NO | `smallRNA_100K.fastq.gz` exists | TODO | Adapter detection coupling |
| `--bgiseq` | NO | TBD | TODO | Probed in v2.x auto-detection but Perl flag still works |
| `--stranded_illumina` | NO | TBD | TODO | Explicit-only in both |
| `--implicon` | NO — only smoke-tested | `BS-seq_10K_R{1,2}.fastq.gz` (re-used) | TODO | Surprising gap; specialty mode |
| `--hardtrim3` | NO | `illumina_10K.fastq.gz` | TODO | Sibling of `--hardtrim5` which IS covered |
| `--polyA` | NO | `PolyA.fastq.gz`, `polyAT_R{1,2}.fastq.gz` | TODO | Compare; some divergence likely (v2.x reworked poly-A) |
| `--retain_unpaired` | Partial — file existence only | `BS-seq_10K_R{1,2}.fastq.gz` | TODO | Content-md5 not asserted today |
| `--rename` | NO | TBD | TODO | Read-ID modification path |
| `--basename` | NO | TBD | TODO | Output naming |
| `--cutadapt_args` | NO | TBD | TODO | Pass-through to Cutadapt |
| `--nextseq N` / `--2colour N` | NO | TBD | TODO | Replaces `-q` |
| `--no_poly_g` | NO | TBD | TODO | Opt-out for v2.x auto poly-G; the validation `demux` step uses this for byte-identity |
| `-a SEQ1 -a SEQ2` (multi-adapter) | NO | `multi_adapters.fa` exists | TODO | Repeated-flag form is v2.x-specific syntax |
| `-a A{N}` shorthand | NO | TBD | TODO | Single-adapter expansion |
| `--max_n` (fractional) | NO | `4_seqs_with_Ns.fastq.gz` exists | TODO | v0.6.8 added fractional |
| `--length` non-default | NO | TBD | TODO | |
| `--quality` non-default | NO | TBD | TODO | |
| `--cores N > 1` | Indirect | several | TODO | Multi-core determinism is Audit §B.5; but Perl didn't have parallel mode, so direct comparison is `--cores 1` only |
| (combinations: e.g., `--paired --rrbs --small_rna`) | NO | TBD | TODO | Exponential surface; pick high-value combos |

**Estimated entries**: ~30 single-flag rows, plus ~10 important combinations. Each row is a small differential CI step or `cargo test` invocation.

## Phase 2 — Edge-case differential corpus

**Template — populate over future sessions.**

Edge cases to write minimal fixtures for, then run through both Perl and Rust:

| Edge case | Fixture sketch | Likely behavior |
|---|---|---|
| Empty file | 0 bytes | Both should fail-fast with clear error |
| Single-read file | 1 record | Both should produce 1-record output |
| All-N read | record with `seq = "NNNN…"` | Filter behavior under `--max_n`, etc. |
| Read length 0 | record with empty seq | Both should reject or handle uniformly |
| Very long read (>1000 bp) | synthetic 5000 bp record | Memory / behavior parity |
| Phred+64 quality encoding | record with quality bytes ≥ 64 | Both should detect or both reject |
| Non-ASCII chars in read ID | UTF-8 in @-line | Pass-through expected; verify |
| Windows line endings | CRLF in headers (note: gzip is binary, but underlying text matters) | Tolerance? |
| BGZF (Bgzip) input | Bgzip-flavoured gzip from samtools | flate2 handles BGZF; does Perl's? |
| Quality line all-min / all-max | min: 0x21 (`!`), max: 0x7E (`~`) | Trim behavior at extremes |
| Multi-member gzip input | concat of two gzip streams | Audit Part 5 §5.1 covers reader; verify Perl agrees |
| Trailing-newline absence | file without final `\n` | Both should accept |

For each: produce small fixture (≤1 KB), commit to `test_files/edge_cases/` (or similar), add a Phase 2 differential CI step.

## Phase 3 — Differential property testing

**Sketch — to expand once Audit §A.1 lands.**

```rust
// tests/parity_proptest.rs (new)
proptest! {
    #[test]
    fn rust_output_matches_perl(input in valid_fastq_strategy()) {
        let rust_out = run_rust(&input)?;
        let perl_out = run_perl(&input)?;   // calls trim_galore_perl via Command
        // Either byte-equal, or the diff is in the registered intentional list
        prop_assert!(equivalent_modulo_known_divergences(&rust_out, &perl_out));
    }
}
```

Notes:

- `valid_fastq_strategy()` produces gzip-encoded valid 4-line records with constrained sequence/quality alphabets.
- `equivalent_modulo_known_divergences` is the load-bearing predicate — its definition IS the parity spec from §"Definition of parity" above. Keep it in sync.
- Run as a CI nightly job (slow), not on every PR. Shrunk failures are committed to `test_files/edge_cases/` as new Phase 2 entries.

## Phase 4 — Differential fuzzing

**Sketch — to expand once Audit §A.2 lands.**

`fuzz/fuzz_targets/parity.rs`:

```rust
fuzz_target!(|data: &[u8]| {
    let rust_result = run_rust_or_err(data);
    let perl_result = run_perl_or_err(data);
    match (rust_result, perl_result) {
        (Ok(r), Ok(p)) => assert!(equivalent_modulo_known_divergences(&r, &p)),
        (Err(_), Err(_)) => (),                        // both reject — fine
        (Ok(_), Err(e)) | (Err(e), Ok(_)) => panic!("parity break: {e:?}"),
    }
});
```

Different from Phase 3: feeds *arbitrary bytes*, not valid FASTQ. Catches "Rust accepts something Perl rejects" or vice-versa, which is a class of parity bug worth knowing about.

## Triage protocol

For each discrepancy the hunt finds:

1. **Reproduce locally** with the offending fixture (Audit §C.4 makes this free; failure-artifacts are downloadable).
2. **Classify**:
   - Intentional v2.x improvement → append to "Definition of parity" → Known intentional v2.x divergences (above) AND mirror in `CHANGELOG.md`.
   - Unintentional / fix in Rust → file an issue with the fixture, link this doc; assign to a future session.
   - Unintentional / Perl-side bug that v2.x correctly handles → document in `CHANGELOG.md` as "behaviour deliberately diverges from Perl 0.6.x bug X"; not a fix target.
3. **Add as a regression guard** to either:
   - The validation matrix (CI step), if reproducible in shell.
   - A unit test with the `// ── regression: ──` convention (Audit §13), if the bug is internal-state.
4. **Update the flag-coverage matrix** (Phase 1 above) with the resolution.

## Out of scope for this plan

- **Performance parity.** Separate workstream; tracked under Audit §B.6.
- **Memory parity.** Separate workstream; tracked under Audit §B.7.
- **Build / reproducibility parity.** Already handled by the `reproducibility` CI job.
- **`--help` text differences.** Separate; tracked under Audit §B.4.
- **Differences in error messages from invalid input.** Subset of behavioural deviations; document case-by-case rather than a parity hunt.

## Open questions for the project lead

To resolve before techniques 3–4 launch:

1. **Where does the parity spec live long-term?** This doc, the `CHANGELOG.md`, the docs site at `docs/src/content/docs/reference/migration/`, or duplicated across all three?
2. **Is the threshold for "intentional" decided per-discovery, or is there a default?** E.g., if v2.x emits a slightly different number of decimal places in a percentage, is that automatically intentional ("v2.x is more precise") or does each one need a Felix sign-off?
3. **How far back does the parity hunt go?** Only against 0.6.11, or also 0.6.10 / 0.6.9 / older? (Probably 0.6.11 only — the others have known bugs that 0.6.11 fixed.)
4. **Cutadapt itself isn't deterministic in all cases.** If Perl-mode produces different output across Cutadapt patch versions on the same input, what's the policy?
5. **For multi-pair invocations**, Perl 0.6.x detected adapters on `$ARGV[0]` only; v2.x detects per-pair. So byte-identity is impossible when adapter detection differs across pairs of mixed library types. Does the parity spec acknowledge this as "byte-identical when each pair is the same library type, otherwise expected divergence"?

## Appendix — references

| | |
|---|---|
| Sister doc (testing audit) | [docs/plans/2026-04-28_REVIEW_ci-cd-audit.md](2026-04-28_REVIEW_ci-cd-audit.md) |
| Validation matrix (5 protected paths) | `.github/workflows/ci.yml:184-499` |
| Documented v2.x divergences (canonical) | `CHANGELOG.md` "Unreleased" + "Documentation since v2.1.0-beta.5" |
| **Perl 0.6.11 source — IN THIS REPO** | `origin/master:trim_galore` (also `fork/master:trim_galore`); commit `dcd108a` = `0.6.11` tag |
| URL form (used by `ci.yml:159` today; replace per Audit §D.1) | <https://raw.githubusercontent.com/FelixKrueger/TrimGalore/0.6.11/trim_galore> |
| Future home of Perl source on optimus_prime (post-GA, per CHANGELOG line 238) | `legacy/trim_galore` |
| Migration notes for users | `docs/src/content/docs/reference/migration/` |
