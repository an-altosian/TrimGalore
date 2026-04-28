# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Trim Galore — Oxidized Edition — is a Rust rewrite of the original Perl Trim Galore (the v0.6.x script lives upstream and is no longer in this repo). It is a single-binary, single-pass adapter and quality trimmer for NGS FASTQ data. There are no external runtime dependencies: adapter detection, alignment, quality trimming, filtering, gzip compression, **and** FastQC reporting (via the bundled `fastqc-rust` library) all run in-process. No Java, no Python, no Cutadapt, no external `fastqc`.

The active development branch is `optimus_prime`; `master` is retained for the legacy Perl release line. The CI workflow `validation` job md5-compares Oxidized output against Perl Trim Galore 0.6.11 (installed from raw GitHub) for several core flag combinations — preserving byte-identity to v0.6.11 is a hard invariant for those flag paths.

## Active release stream

Stable on crates.io: **v2.0.0**. Current branch is **v2.1.0-beta.5** with a pre-GA validation pass underway via nf-core. Recent fixes (queued for `beta.6`, tracked in `CHANGELOG.md` under "Unreleased"):

- RRBS `Total written (filtered)` bp accounting — `TrimResult` now carries a `bp_after_cutadapt` field captured before RRBS / poly-A/G / N-trim / clipping, restoring byte-for-byte parity with v0.6.x for that line. Trimmed FASTQ output is unchanged; only the reported count.
- `RUN STATISTICS` filter-removed lines (`too_short`, `too_long`, `too_many_n`, paired `pairs_removed_n`) are now always emitted even at zero — MultiQC's canonical fallback parser greps for the literal line and treats absence as a parse failure.

When changing report-text formatting, check `CHANGELOG.md`'s "Unreleased" block first — the v2.x report intentionally diverges from Perl v0.6.x in four documented places (RRBS quality-trim line shape, dropped adapter family-name annotation, omitted "Bases preceding removed adapters" histogram, modern Cutadapt `floor(L × error_rate)` `max.err` formula). The validation matrix's md5s are pinned to the v2.x shape.

## Build, lint, test

Requires Rust toolchain (the crate's `rust-version` floor is `1.88`; CI uses `dtolnay/rust-toolchain@stable`). Edition is 2024.

```bash
cargo build --release          # binary lands at target/release/trim_galore
cargo test                     # unit tests (cwd must be crate root — fixtures resolve relative to test_files/)
cargo test <name>              # run a single test by name substring
cargo fmt --all -- --check     # CI fails on unformatted code
cargo clippy --all-targets --release -- -D warnings   # CI fails on any clippy warning
```

To reproduce the `validation` CI job locally (md5-compare against Perl Trim Galore 0.6.11), see the steps in `.github/workflows/ci.yml` — it installs the Perl `trim_galore` from a pinned upstream URL via conda and diffs gzip-decompressed outputs.

### Reproducible builds

`build.rs` reads `SOURCE_DATE_EPOCH` to stamp the binary with a deterministic build timestamp; missing → wall-clock time, malformed → hard panic. The CI `reproducibility` job builds twice with the same `SOURCE_DATE_EPOCH` and asserts the resulting binaries are bit-identical. Don't introduce wall-clock time, hostnames, or absolute paths into the release binary.

`./target/release/trim_galore --version` (long form) must emit a provenance line `<git-hash> — <os>/<arch> — built <ISO-8601-UTC>` (literal em-dash); `-V` must NOT include that line. CI greps for both. The format is constructed in `build.rs` and printed via `env!("VERSION_BODY")`.

## Test fixtures

`test_files/` holds gzipped FASTQ fixtures used by both `cargo test` and the CI validation matrix. Tests in `src/cli.rs` reference these by relative path, so `cargo test` must be run from the crate root.

| Fixture | Exercises |
|---------|-----------|
| `BS-seq_10K_R{1,2}.fastq.gz` | BS-seq paired-end |
| `SRR24766921_RRBS_R{1,2}.fastq.gz` | RRBS (`--rrbs`) |
| `clock_10K_R{1,2}.fastq.gz` | `--clock` specialty mode |
| `demux_test.fastq.gz` + `demux_test_samplesheet.txt` | 3' inline barcode demux |
| `nextera_100K.fastq.gz` + `_trimming_report.txt` | Nextera adapter detection + golden report parity |
| `smallRNA_100K.fastq.gz` (+ `_R2`) + `_trimming_report.txt` | smallRNA adapter detection + golden report parity |
| `multi_adapters.fa` | `-a file:` FASTA-loading path |
| `4_seqs_with_Ns.fastq.gz` | max-N (`--max_n`) filter |
| `polyAT_R{1,2}.fastq.gz`, `PolyA.fastq.gz`, `PolyT.fastq.gz`, `illumina10K_with_polyA.fastq.gz` | poly-A / poly-T trimming, paired and single-end |
| `illumina_10K.fastq.gz`, `10K_150bp.fastq.gz` | general Illumina, longer-read scenarios |
| `colorspace_file.fastq`, `truncated.fq.gz`, `empty_file.fastq` | negative cases — `FastqReader::sanity_check` should fail loudly on each |

The `*_trimming_report.txt` files committed alongside `nextera_100K` and `smallRNA_100K` are golden references — diff against them when changing report formatting, and update intentionally with a `CHANGELOG.md` entry.

## Architecture

Single binary (`src/main.rs` is the only `[[bin]]`); the rest is a library crate (`src/lib.rs`) so unit tests can reach internals. `main()` parses CLI, runs a sanity check on the first input, then dispatches:

1. **Specialty modes** (run-and-exit, bypass the trimming pipeline): `--hardtrim5`, `--hardtrim3`, `--clock`, `--implicon`. All four accept multi-pair input (an even number of files; per-pair "Pair N of M" headers + the same output-collision pre-flight that `--paired` runs).
2. **Paired mode** (`--paired`): consecutive files form R1/R2 pairs. Before any I/O, a pre-flight hashes prospective output paths case-folded (ASCII lowercase) so collisions on APFS/NTFS — and case-only aliases — fail loudly rather than silently overwriting. Adapter auto-detection runs **per pair** (intentional deviation from Perl v0.6.x, which detected once on `$ARGV[0]`).
3. **Single-end**: each input is processed independently in a loop.

### Module map (under `src/`)

- `cli.rs` — clap definitions, `rewrite_perl_short_flags()` pre-parser (fixes Perl-era `-r1`/`-r2`), `Cli::validate()` (paired-input sanity, duplicate-pair detection, mutually-exclusive flag conflicts beyond what clap expresses).
- `adapter.rs` — built-in adapter sequences (Illumina `AGATCGGAAGAGC`, Nextera `CTGTCTCTTATA`, smallRNA `TGGAATTCTCGG`, BGI/DNBSEQ, stranded Illumina) and auto-detection by scanning the first 1 M reads. BGI is probed; `--stranded_illumina` stays explicit-only because its sequence is ambiguous with Nextera.
- `alignment.rs` — semi-global unit-cost DP that re-implements Cutadapt's adapter matching algorithm. Byte-for-byte parity with Cutadapt is what makes the `validation` CI job pass.
- `quality.rs` — BWA-style 3' quality trimming (Li & Durbin 2009; matches Cutadapt's algorithm).
- `trimmer.rs` — orchestrator: wires quality trim → adapter trim → clip → filter → report for both single- and paired-end. Public `run_single_end` / `run_paired_end` entry points used by `main.rs`.
- `parallel.rs` — `--cores N` worker pool. Each worker trims **and** gzip-compresses its own chunk, and the chunks are concatenated in order — RFC 1952 permits gzip-member concatenation, so the output is a valid `.gz` file. This replaces the older readers→main→writers pipeline.
- `fastq.rs` — `FastqReader`/`FastqWriter` with gzip awareness and 64 KB buffered I/O; `FastqReader::sanity_check` is the entry-side guard against truncated/empty/colorspace input.
- `filters.rs` — length, max-length, max-N, and unpaired-rescue filters.
- `io.rs` — output naming: `*_trimmed.fq(.gz)` (SE), `*_val_{1,2}.fq(.gz)` (PE), `*_unpaired_{1,2}.fq(.gz)`, `*_trimming_report.txt` + `*_trimming_report.json`.
- `demux.rs` — 3' inline barcode demultiplexing (single-end only, matching the original).
- `specialty.rs` — `--hardtrim5/3`, `--clock` (Epigenetic Clock UMI), `--implicon` (UMI from R2). Each mode owns its output naming.
- `fastqc.rs` — bundled FastQC integration via the `fastqc-rust` crate (exact-pinned to `=1.0.1`; treat upstream bumps as deliberate test events). `--fastqc_args` accepts a curated subset of flags; unknown flags warn-and-ignore for forward compatibility.
- `report.rs` — generates the MultiQC-compatible text + JSON trimming reports.

### Adapter shorthand

`-a` / `-a2` accept `A{N}` shorthand (e.g. `-a A{10}` → `AAAAAAAAAA`), repeatable for multi-adapter (`-a SEQ1 -a SEQ2`), or `file:adapters.fa` to load from a FASTA file. The Perl-era embedded-string syntax (`-a " SEQ -a SEQ"`) still parses for back-compat.

## Bundled FastQC dependency

`fastqc-rust` is exact-pinned (`=1.0.1`). The pin is deliberate: the crate is brand-new (v1.0.0 published 2026-04-26) and any bump should be taken as a deliberate-test event with output-byte-identity re-verified against Java FastQC 0.12.1. The CI `validation` job has a smoke test that asserts (a) `fastqc` is **not** on `$PATH` (so a green run proves the bundled library did the work) and (b) the produced `.zip` contains `summary.txt`, `fastqc_data.txt`, `Images/`, `Icons/`.

The `--fastqc_args` parser in `src/fastqc.rs` accepts only this curated subset (everything else warns and is ignored, so old wrapper scripts pass through cleanly):

| Flag | Effect |
|------|--------|
| `--nogroup` | Don't group bases above 50 bp |
| `--expgroup` | Expand grouping past 50 bp |
| `--quiet` | Suppress progress messages |
| `--svg` | Emit SVG plots in addition to PNG |
| `--nano` | Process Oxford Nanopore long-read data |
| `--nofilter` | Disable quality pre-filtering |
| `--casava` | Treat input as Casava-format files |
| `-t`, `--threads N` | FastQC analysis thread count |
| `-o`, `--outdir DIR` | Output directory for FastQC reports |

## CI workflows

Three workflows under `.github/workflows/`:

- **`ci.yml`** runs on every push, every PR, and on a nightly `schedule:`. Five jobs:
  - `rust-tests` — `cargo test` on Ubuntu. Note the release profile (`lto = true`, `codegen-units = 1`) means release builds behave subtly differently from debug; the validation matrix below uses release.
  - `reproducibility` — builds the binary twice with the same `SOURCE_DATE_EPOCH` and asserts byte-identical output. Don't introduce wall-clock time, hostnames, or absolute paths into the release binary.
  - `lint` — `cargo fmt --all -- --check` + `cargo clippy --all-targets --release -- -D warnings`. New clippy warnings fail CI.
  - `audit` — `cargo audit` for known CVEs in dependencies.
  - `validation` — installs Perl Trim Galore 0.6.11 from a pinned upstream URL via conda and md5-compares Oxidized output against it for SE, PE, hardtrim5, clock, and demux paths. Also asserts `fastqc` is **not** on `$PATH` and that the produced FastQC `.zip` contains the expected entries.
- **`docs.yml`** auto-deploys the Astro Starlight docs site to GitHub Pages on push-to-`master` that touches `docs/`, `CHANGELOG.md`, or the workflow itself. Two jobs: `build` (`npm ci && npm run build` in `docs/`) and `deploy` (uploads `docs/dist/` as the Pages artifact).
- **`release.yml`** is tag-driven (`workflow_dispatch` also). Pipeline: `check-release` → matrix `build-binaries` (Linux x86_64/aarch64, macOS Apple Silicon) → `smoke-test-binaries` → matrix `docker-build` (amd64/arm64) → `docker-merge` (multi-arch manifest to `ghcr.io/felixkrueger/trimgalore`) → `smoke-test-docker` → `create-tag-and-release` (GitHub release) → `upload-binaries` → `publish-crate` (crates.io).

## Documentation site

`docs/` is an Astro Starlight site published at <https://felixkrueger.github.io/TrimGalore/>. It's its own npm package — not part of the Rust crate (it's in `Cargo.toml`'s `exclude` list).

```bash
cd docs
npm install
npm run dev      # http://localhost:4321/TrimGalore/
npm run build    # static build into docs/dist/
npm run preview  # serve docs/dist/ locally
```

The release notes page at `docs/src/content/docs/reference/changelog.md` is a copy of the top-level `CHANGELOG.md` with a Starlight frontmatter block prepended and one inline image path rewritten — keep them in sync at release. Two design docs (`docs/DESIGN.md`, `docs/PRODUCT.md`) are `.gitignore`-d on purpose; they're internal scratch.

## Distribution

- **crates.io**: `cargo install trim-galore` (note hyphen — the binary is `trim_galore` with underscore). `Cargo.toml`'s `exclude` list (`docs/`, `.github/`, `test_files/`, `plans/`, `.claude/`, `CLAUDE.md`, `CHANGELOG.md`) keeps the published tarball compact.
- **bioconda**: `conda install -c bioconda trim-galore` (recipe maintained downstream).
- **Docker**: multi-stage `Dockerfile` (`rust:1.88-bookworm` builder → `debian:bookworm-slim` runtime, only `procps` + `ca-certificates` added). Multi-arch (amd64+arm64) images on `ghcr.io/felixkrueger/trimgalore`. No Java, no FastQC tarball, no Perl in the runtime image — `fastqc-rust` is statically linked into the binary.
- **Prebuilt binaries**: Linux (x86_64, aarch64) and macOS (Apple Silicon) on the GitHub Releases page. Intel Mac is `cargo install` only (no prebuilt artifact).

## Conventions worth knowing

- **No external runtime deps.** Anything that would shell out to `cutadapt`, `pigz`, `fastqc`, or `java` is a regression — the v2.x story is "single static binary".
- **CI is `-D warnings`.** New clippy warnings will fail CI; fix them rather than `#[allow]`-ing without justification.
- **Validation matrix is load-bearing.** The CI `validation` job md5-checks Oxidized output against Perl 0.6.11 for SE, PE, hardtrim5, clock, and demux. If you change any of those code paths, expect to either preserve byte-identity or update the validation job with an explicit reason.
- **Output-collision pre-flight.** Don't bypass it — it catches issue #216 (case-only aliases on APFS/NTFS silently overwriting).
- **Em-dashes in user-facing strings.** `--version` provenance uses literal em-dashes; CI grep is content-targeted on that character.
- **Pure-Rust gzip stack.** `flate2` is configured with `default-features = false, features = ["zlib-rs"]` and `gzp` with `features = ["deflate_rust"]` — no system zlib linkage, which is what makes the binary truly static. Don't switch back to `miniz_oxide` defaults or to a `libz-sys` feature without a benchmark.
- **Specific-file `git add` only.** `.claude/`, `/plans/`, `/legacy/`, `.nf-test/`, `docs/DESIGN.md`, and `docs/PRODUCT.md` are `.gitignore`-d on purpose. Never `git add -A` or `git add .` from the repo root — it's safe today but the safety relies on the ignore list staying in sync.
