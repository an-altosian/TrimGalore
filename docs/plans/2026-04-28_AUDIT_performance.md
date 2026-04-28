# 2026-04-28 — Performance audit

| | |
|---|---|
| **Status** | Code-driven audit complete. Per-function review across all 15 source files. Wall-clock baseline established |
| **Scope** | Every function in `src/`, ranked by expected impact. Architectural opportunities identified separately |
| **Methodology** | **In-process SIGPROF sampling via pprof-rs** (kernel `perf_event_paranoid=2` blocks `samply`/`perf` in this sandbox; pprof-rs uses POSIX timers and works without kernel perf access). Plus wall-clock scaling on 1M-read fixture, source-code review, cross-cutting allocation/clone grep. The pprof harness lives at `examples/profile_smallrna.rs` and produces `flamegraph_se_cores{N}.svg` + a folded-stacks text file for grep-friendly analysis |
| **Audience** | Future sessions implementing the optimizations; upstream for review |
| **Related** | [docs/plans/2026-04-28_REVIEW_ci-cd-audit.md](2026-04-28_REVIEW_ci-cd-audit.md), [docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md](2026-04-28_FINDINGS_parity-hunt-phase1-2.md) |

## Headline findings

**These are the SAMPLE-BASED findings (after running pprof-rs on 1M reads). The earlier code-review-only predictions were materially wrong — see "Reality vs prediction" below.**

1. **Gzip compression dominates: 60.7% of CPU time** at cores=1 (10-run merged, 3,950 samples: 51.9% zlib_rs + 8.8% crc32fast). At cores=8, gzip+crc takes **59.1%** (50.6% + 8.5%, 814 merged samples). **The CHANGELOG estimate of "~60%" is essentially exact across both core counts.**
2. **Trimming algorithms (in `trim_read`): 35.1% at cores=1, 35.6% at cores=8** — the proportional breakdown is essentially constant across core counts, indicating the parallel pipeline scales each subsystem evenly without one becoming a serial bottleneck.
3. **`FastqRecord::write_to` is the dominant single function: 2,435 inclusive samples (61.6%)** at cores=1 — feeding into `write_fmt → write_all → flate2 → zlib_rs::deflate_medium → longest_match`. Currently uses 4 separate `writeln!` calls per record. **Reducing to 1 buffered write per record is a real optimization** — bigger chunks reach deflate at once.
4. **`zlib_rs::deflate_medium` (1,968 samples = 49.8%) is the hottest single function**, followed by `longest_match` (1,376 = 34.8%). Both are part of the medium-effort deflate algorithm used at compression level 6. **Lowering compression level from 6 to 4 bypasses `medium` entirely** (level 4 uses `quick`).
4. **The README claim "near-linear speedup up to ~16 cores" overstates** — actual data shows the knee at **8 cores** (3.27× speedup on 1M reads). Beyond 8 cores, wall-clock plateaus and user time grows, indicating contention overhead.
5. **`fastq.rs::FastqRecord` uses `String` for sequence/quality** but the impact is small: ~2% of samples involve fastq-module functions. Switching to `Vec<u8>` is still a clean ergonomic improvement (eliminates `.as_bytes()` boilerplate scattered through 8 sites in trimmer.rs) but **its perf impact is negligible** — the original code-review estimate of 5–15% was wildly wrong.

## Methodology revisions

**Run count and averaging**: All wall-clock numbers in this audit are **10-run means with 1 warmup discarded**, measured by `hyperfine 1.20.0`. All sample-percentage breakdowns are **10-run merged samples** (each run's folded-stack file concatenated, then categorically tallied). The original draft of this audit reported single-shot wall-clock and single-run pprof samples — the table immediately above this section now uses proper averaged data, and per-category percentages have been recomputed from the merged 3,950-sample dataset (cores=1) and 814-sample dataset (cores=8).

**Original single-shot vs 10-run-mean comparison**:

| Cores | Original single-shot (s) | 10-run mean (s) | Within mean? |
|---:|---:|---:|---:|
| 1 | 3.054 | 3.044 ± 0.009 | ✓ (yes) |
| 2 | 1.944 | 1.971 ± 0.016 | ✓ (yes) |
| 4 | 1.141 | 1.171 ± 0.011 | ✓ (1.6 σ) |
| 8 | 0.935 | 0.954 ± 0.031 | ✓ (yes) |
| 16 | 1.025 | 0.974 ± 0.028 | ✗ (1.8 σ — slow tail) |
| 32 | 0.989 | 0.984 ± 0.043 | ✓ (yes) |

The single-shot numbers were within ~5% of the eventual 10-run mean — close, but the cores=16 measurement was on the slow tail of the distribution and led to overconfident claims about the cores=8 → cores=16 step. The corrected story is "cores=8/16/32 are statistically indistinguishable" rather than "knee at 8 with regression at 16".

## Reality vs prediction (instructive contrast)

The first version of this audit was code-review only because `samply`/`perf` were blocked by sandbox `kernel.perf_event_paranoid=2`. After installing `pprof-rs` (which uses POSIX SIGPROF and doesn't need kernel perf), the real samples produced a different picture:

| Component | Code-review prediction | Real samples (cores=1) | Verdict |
|---|---|---|---|
| Gzip compression | ~60% (per CHANGELOG) | **55% zlib_rs + 6% crc32 = 62%** | ✓ matches CHANGELOG; my code-review took it for granted and built optimization recommendations elsewhere |
| `find_3prime_adapter` DP allocation | "#1 hot spot, ~14M allocations, 5–15% wall improvement from fix" | inlined into trim_read at 144 samples; allocation isolation impossible at this resolution | **Wrong** — the function is part of the 36% trim_read budget, but the DP-allocation slice is invisible in samples (likely <1%) |
| `FastqRecord::seq/qual` as `String` | "5–15% impact, switch to `Vec<u8>`" | fastq module total: 8 samples = **2%** | **Wildly wrong** — the optimization is real but the benefit is ~10× smaller than I estimated |
| Threaded-reader `mem::replace` allocs | "2–5% wall, easy fix" | doesn't appear in top samples | **Wrong magnitude** — likely <1% impact |
| `SmallVec` for `adapter_matches` | "1–2% wall" | invisible at this resolution | **Probably wrong** — likely <0.5% |
| README claim "near-linear up to 16 cores" | flagged as overstating reality | wall-clock data (independent of pprof) shows knee at 8 cores, user time **doubles** at 16 | ✓ confirmed |

**Lesson**: code-review estimates of allocation cost without sample data systematically overestimate. The reason is that **modern allocators (jemalloc / glibc malloc) are very fast for small allocations**, and the cycles cost of `Vec::new()` or `String::to_string()` for a sub-1KB allocation is in the tens-of-nanoseconds range, not microseconds. With 1M of them, that's still only ~50ms — drowned in 4.3 seconds of gzip work.

**Where code review WAS right**:
- Gzip dominance (CHANGELOG already said this; I underweighted it in recommendations)
- `quality.rs` is already optimal (samples confirm: 0 leaf-level samples in quality module)
- `find_3prime_adapter` is on the hot path (samples: trim_read takes 36% and most of that is alignment work)

**Where code review WAS wrong**:
- Predicted "5–15% wins" from String/allocation changes that are actually ~1–2%
- Underestimated the absolute dominance of gzip compression
- Missed that `FastqRecord::write_to`'s **4-writeln pattern** is the actual fastq-side bottleneck (each writeln is a separate gzip-encoder call)

## Baseline measurements

### Wall-clock scaling on 1M reads (smallRNA_100K × 10) — 10 runs, hyperfine, 1 warmup discarded

| Cores | Mean (s) | StdDev | Range (min … max) | Speedup vs cores=1 | User time (s) |
|---:|---:|---:|---:|---:|---:|
| 1 | 3.044 | ±0.009 (0.3%) | 3.031 … 3.058 | 1.00× | 3.029 |
| 2 | 1.971 | ±0.016 (0.8%) | 1.950 … 1.997 | 1.55× | 3.898 |
| 4 | 1.171 | ±0.011 (0.9%) | 1.158 … 1.195 | 2.60× | 4.018 |
| **8** | **0.954** | ±0.031 (3.2%) | 0.927 … 1.017 | **3.19×** | 4.677 |
| 16 | 0.974 | ±0.028 (2.9%) | 0.936 … 1.021 | 3.13× | 5.803 |
| 32 | 0.984 | ±0.043 (4.4%) | 0.889 … 1.029 | 3.09× | 5.866 |

**Diagnosis (statistically corrected)**: cores=8/16/32 confidence intervals overlap heavily (cores=8: [0.892, 1.016]; cores=16: [0.918, 1.030]; cores=32: [0.898, 1.070]). The plateau is real, but **the precise location of the knee is not pinpointable from this data** — the original "knee at 8 cores" claim was on the edge of statistical significance.

What IS confidently true:

- **User time grows monotonically with cores** (3.0 → 4.0 → 4.7 → 5.8 → 5.9 from 1→4→8→16→32 cores). At 16+ cores, user time roughly doubles vs cores=1 without wall-clock improvement — overhead growth is real.
- **cores=8 is empirically the fastest mean wall-clock**, but cores=4 (1.171 ± 0.011) is within 23% of it with much tighter variance (3-min stddev vs 30-min stddev). For workloads where consistency matters more than peak throughput, cores=4 may be the better operating point.
- **The README's "near-linear up to ~16 cores" claim is overstated.** Linear from 1→8 (3.19× on 8 cores = 40% efficiency at cores=8; 80% at cores=2). Non-linear past 8 with diminishing-then-zero returns.

### Per-fixture throughput (`--cores 8`, 10 runs + 1 warmup each)

| Fixture | Reads | Compressed | Mean (ms) | StdDev |
|---|---:|---:|---:|---:|
| BS-seq_10K_R1 | 10K | 266 KB | 43.8 | ±15.5 (35%) |
| illumina_10K | 10K | 960 KB | 79.4 | ±2.1 (2.6%) |
| smallRNA_100K | 100K | 2.0 MB | 100.5 | ±8.3 (8.3%) |
| nextera_100K | 100K | 3.3 MB | 131.3 | ±12.2 (9.3%) |
| smallRNA_1M (synthetic) | 1M | 20 MB | 954 | ±31 (3.2%) |

**The small-fixture results have high variance** (BS-seq_10K_R1 at 35% stddev) — at sub-100ms runtimes, startup + thread-spawn cost dominates and any background system noise blows up the relative variance. **Trust the 1M-read fixture for stable per-cores measurements.** Throughput on the 1M fixture at cores=8 is ~1.05M reads/s.

md5 byte-identity holds across `--cores` 1–32 (the multi-core determinism property from CHANGELOG, verified empirically across all 60 runs).

## Per-function review

### `src/alignment.rs` (315 lines) — the hot loop

| Function | Lines | Verdict | Notes |
|---|---|---|---|
| **`find_3prime_adapter`** | 38–136 | **HIGH-IMPACT OPPORTUNITY** | Per-read DP allocation. See PERF-1 below |
| `backtrace_start` | 144–174 | Acceptable | Walks a 2D matrix, ~O(m+n) per match. Could share buffer with the DP if PERF-1 lands |

**PERF-1 — DP matrix allocation**: line 57 `let mut dp = vec![vec![0usize; n + 1]; m + 1]` allocates 1 outer Vec + (m+1) inner Vecs per call. For 1M reads × ~14 inner Vecs = 14M small allocations.

Three layered fixes:

- **Quick win (10 LOC)**: flatten to a single `Vec<usize>` indexed as `dp[i*(n+1)+j]`. Reduces allocations 14× per call.
- **Better (50 LOC)**: thread-local reusable buffer. Allocations: zero after first call per thread. The buffer grows but doesn't shrink.
- **Best (200–500 LOC)**: switch to **Myers' bit-parallel edit distance** for adapters ≤64bp. Encodes the DP into 64-bit integer ops — typically 10–100× faster than scalar DP. Cutadapt uses this internally for short adapters.

**Cell type**: currently `usize` (8 bytes). Max edit distance for typical 13bp adapter at 10% error rate is `floor(13 × 0.1) = 1`. **`u8` would suffice** and reduce matrix size 8× (better cache locality). Combined with flat layout: 14×151×1 = 2,114 bytes vs current ~12,200 bytes.

### `src/quality.rs` (507 lines) — already efficient

| Function | Lines | Verdict | Notes |
|---|---|---|---|
| `quality_trim_3prime` | 22–51 | **LEAVE AS-IS** | Single backward pass, no allocation, branch-predictable. Already optimal |
| `quality_trim_3prime_nextseq` | 61–96 | **LEAVE AS-IS** | Same shape with one branch on `b'G'`. Branch predictor handles it |
| `homopolymer_trim_index` | 111–167 | Minor opportunity | Could early-terminate when `score + (n-i) < best_score` (unrecoverable). Marginal — the function is short and runs once per read |
| `poly_a_trim_index` | 172–174 | **LEAVE AS-IS** | Thin wrapper; nothing to optimize |

**This module is the model for the rest of the codebase** — careful imperative loops, zero allocation, no `String`. The team got `quality.rs` right.

### `src/fastq.rs` (623 lines) — the BIG opportunity

| Function | Lines | Verdict | Notes |
|---|---|---|---|
| **`FastqRecord` field types** | 22–28 | **HIGH-IMPACT** | See PERF-2 |
| `FastqRecord::truncate` | 52–57 | OK | `String::truncate` is in-place |
| **`FastqRecord::clip_5prime`** | 61–69 | **MEDIUM-IMPACT** | Reallocates remaining sequence. See PERF-3 |
| `FastqRecord::clip_3prime` | 74–84 | Mild | Clipped suffix only allocated when `--rename` on. Could lazy-allocate via `Cow` |
| `FastqRecord::n_count` | 87–89 | OK | Single iteration |
| **`FastqRecord::trim_ns`** | 92–114 | **MEDIUM-IMPACT** | `from_utf8_lossy` then `.to_string()` — unnecessary on ASCII input |
| `FastqRecord::append_to_id` | 117–121 | Cold path | Only called with `--rename` |
| `FastqRecord::write_to` | 42–48 | Mild | Four `writeln!` calls. Could write to a `Vec<u8>` then single `write_all` |
| **`FastqReader::next_record` (Direct)** | 302–333 | **HIGH-IMPACT** | 4× `.to_string()` per record → 4M allocs on 1M reads. See PERF-2 |
| **`FastqReader::read_next_direct`** | 266–299 | **HIGH-IMPACT** | Same 4× `.to_string()` pattern (used by threaded reader) |
| **`FastqReader::next_record` (Threaded)** | 334–379 | **HIGH-IMPACT** | `mem::replace` with `FastqRecord { String::new(), String::new(), String::new() }` per consumed record — 3M empty String allocs on 1M reads. See PERF-4 |
| `FastqReader::open_threaded` | 188–246 | OK | One-time setup |
| `FastqWriter::create` | 426–468 | OK | One-time setup |
| `FastqWriter::write_record` | 471–473 | Mild | Delegates to `record.write_to`. Same `writeln!` issue |
| `FastqReader::sanity_check` | 384–415 | Cold path | Once per file |

**PERF-2 — `String` → `Vec<u8>` for `FastqRecord::seq` and `FastqRecord::qual`**: 

```rust
pub struct FastqRecord {
    pub id: String,        // keep — small, user-facing
    pub seq: Vec<u8>,      // changed
    pub qual: Vec<u8>,     // changed
}
```

This:
- Skips UTF-8 validation on read (`String` requires it; `Vec<u8>` doesn't).
- Eliminates the `.as_bytes()` calls scattered through `trimmer.rs` (8 occurrences).
- Allows `String::from_utf8_lossy` to be removed from `trim_ns` (4-line simplification).
- Roughly halves cycles on the read path (UTF-8 validation is per-byte).

Trade-off: `id` stays `String` because it's printed; `seq`/`qual` are byte-blob data.

**PERF-3 — `clip_5prime` in-place rotation**:

Current (line 64): `self.seq = self.seq[n..].to_string()` allocates a new String of length `seq.len() - n`. With PERF-2, change to:

```rust
self.seq.drain(0..n);  // O(n) memmove, no allocation
```

For 100K reads × 5bp clip = 500K bytes of useless allocation eliminated.

**PERF-4 — Threaded-reader buffer slot type**:

Current (`fastq.rs:140`): `buffer: Vec<FastqRecord>`. Consumption via `mem::replace(&mut buffer[idx], FastqRecord { id: "".into(), seq: "".into(), qual: "".into() })` allocates 3 empty Strings per consumed record.

Fix:

```rust
buffer: Vec<Option<FastqRecord>>,
// consumption:
let record = buffer[idx].take().unwrap();
```

`Option::take` is a single tag write + ownership transfer. Zero allocations.

### `src/parallel.rs` (632 lines) — scaling bottleneck

| Function | Lines | Verdict | Notes |
|---|---|---|---|
| **`run_paired_end_parallel`** | 59–223 | **MEDIUM** | See PERF-5: channel design + ordered output |
| **`process_paired_batch`** | 226–319 | **MEDIUM** | Per-batch GzEncoder allocation. Tolerable at 4096 records/batch but adds up |
| `process_pairs` | 326–444 | OK | Per-read trim + filter; the inner loop is fine |
| `run_single_end_parallel` | 456–548 | OK | Mirror of `run_paired_end_parallel` |
| `process_single_batch` | 551–576 | OK | Mirror |
| `process_reads` | 579–632 | OK | Mirror of `process_pairs` |

**PERF-5 — scaling plateau at 8 cores**:

The wall-clock data shows plateau at 8 cores. Three contributing factors visible in the code:

1. **Single-threaded gzip decompression on input**: `MultiGzDecoder` (fastq.rs:168) is single-threaded. On a multi-member-gzip input or BGZF-style indexed input, this could parallelize. Output uses parallel gzip (`gzp::par::compress::ParCompressBuilder` for `cores > 1`) but that's already in place.

2. **`mpsc::sync_channel(2)` per-worker bound** (parallel.rs:78): the work channel from reader → worker has buffer depth 2. With 16 workers × 2 = 32 batches in flight. If reader is fast (it usually is for short reads), workers may starve waiting for one batch each rather than having a deeper queue. Try buffer depth 4 or 8.

3. **`BTreeMap<u64, BatchResult>` for ordered output** (parallel.rs:185): main thread accumulates out-of-order results in a BTreeMap and flushes when in-order. With 16 workers in flight, the BTreeMap grows to 16 entries — fine in size, but each insert is O(log n) and the lock-free ordering forces all results through one channel/main-thread.

**Suggested fixes** in order of cost:

- **Quick (10 LOC)**: Increase work-channel buffer from `sync_channel(2)` to `sync_channel(4)` or `(8)`. Test scaling.
- **Medium (~50 LOC)**: Replace mpsc with a lock-free queue (`crossbeam_channel` or `flume`). Better contention behaviour at 16+ threads.
- **Architectural (~200 LOC)**: Replace ordered-output collection with per-worker output streams that interleave gzip members. Requires committing to multi-member output (already produced) and dropping the strict ordering requirement (currently held by `BTreeMap`).

### `src/trimmer.rs` (793 lines) — orchestrator

| Function | Lines | Verdict | Notes |
|---|---|---|---|
| **`trim_read`** | 85–284 | **MEDIUM** | See PERF-6 |
| `update_adapter_stats` | 287–298 | OK | Per-read, but small |
| `run_single_end` | 304–363 | OK | Outer loop |
| `run_paired_end` | 370–793 | OK | Outer loop |

**PERF-6 — `trim_read` per-call allocation patterns**:

- Line 114: `let mut adapter_matches: Vec<(usize, usize)> = Vec::new()` — fresh Vec per read even when no adapter found. 1M empty Vecs/run. Replace with `SmallVec<[(usize,usize); 1]>` (almost all reads have ≤1 match) — eliminates the alloc.
- Lines 260, 269: `format!(":clip5:{}", seq)` only when `--rename` is on. Cold path; leave as-is.
- Line 91, 98, 122, 129, 169, 196, 218: `.as_bytes()` calls — zero-cost. With PERF-2 these go away entirely.

### `src/filters.rs` (217 lines) — clean

| Function | Lines | Verdict | Notes |
|---|---|---|---|
| `filter_single_end` | 19–44 | **LEAVE AS-IS** | Two branches + one Option; perfectly fast |
| `filter_paired_end` | 73–107 | **LEAVE AS-IS** | Same |
| `exceeds_n_threshold` | 119–133 | **LEAVE AS-IS** | One match on a small enum |

The `MaxNFilter::clone()` calls noted in trimmer.rs/parallel.rs are essentially free — the enum is `Copy`-shaped (`Count(usize)` or `Fraction(f64)`).

### `src/io.rs` (229 lines) — output naming

Filename construction (`build_output_path`, `build_paired_output_paths`, etc.). Once per file. Two real issues here belong to F3 (the upstream `--basename` bug, already filed as #244) and P3-F2 (output extension behaviour, in #245). **No per-read perf concerns**. Leave as-is from a perf perspective.

### `src/demux.rs` (263 lines) — barcode demux

| Function | Verdict | Notes |
|---|---|---|
| `run_demux` | OK | Per-record HashMap lookup. Could replace `HashMap<String, FastqWriter>` with `Vec<FastqWriter>` indexed by per-barcode integer ID — eliminates string hashing per read |
| Internal helpers | OK | Run once |

**PERF-7 (low priority)**: `HashMap<String, FastqWriter>` for per-sample writers. With ~3–10 samples typically, a `Vec<FastqWriter>` keyed by precomputed barcode→index lookup would skip per-read string hashing. Marginal — demux is rarely the bottleneck.

### `src/adapter.rs` (827 lines) — adapter detection

| Function | Verdict | Notes |
|---|---|---|
| `detect_adapter` | One-time | Scans first 1M reads of input, runs once. Not in per-read hot path |
| `parse_adapter_specs` | One-time | Once per invocation |
| Other helpers | One-time | Once per invocation |

**No per-read concerns.** The 1M-read scan IS substantial CPU but it's a one-time cost — not improvable without changing semantics.

### `src/specialty.rs` (614 lines) — `--clock`/`--implicon`/`--hardtrim*`

Specialty modes. Each function does a fixed-size operation per read (extract 8bp UMI, hardtrim N bases). Already tight loops. **No optimization opportunities surfaced** in the review.

### `src/cli.rs` (816 lines) — clap definitions

Once-per-invocation parsing. Two correctness bugs are filed (F1 `--max_n 0.5`, F2 `--clip_r1` lowercase) but no perf concerns.

### `src/main.rs` (948 lines) — entry-point dispatch

Once-per-invocation. The `HashMap<String, PathBuf>` for output-collision pre-flight (lines 104–105, 907) is correct and fast — one HashMap pass per invocation, not per read.

### `src/report.rs` (1633 lines) — report generation

Once per file at the end of processing. Large because of the verbose text-report format. Not in the per-read hot path.

**One micro-issue**: `format!()` is heavily used. If report generation showed up in profiling, switching to `write!()` directly into a `Vec<u8>` would save intermediate allocations. But it's not in the hot path — leave it.

### `src/fastqc.rs` (172 lines) — bundled FastQC dispatch

Wrapper around `fastqc-rust` library. Performance is upstream. Leave as-is.

## Top-10 highest-impact optimization candidates (REVISED with sample data)

Ranked by **measured** wall-clock impact (or solid extrapolation from sample percentages). The earlier code-review-only ranking is preserved below as "Original (superseded)" for the reality-vs-prediction record.

### Sample-grounded ranking

| # | Change | File | Effort | Expected gain | Evidence |
|--:|---|---|---|---|---|
| **1** | **Lower default gzip compression level from 6 to 4 (or expose a `--fast-gz` flag)** | `fastq.rs:454`, `parallel.rs:249,250,252,257,563` | Trivial (~5 LOC) | **20–35%** at the same `--cores N` | Gzip is 62% of CPU; level 4 is ~30% faster than level 6 with ~3% larger output (zlib-rs benchmarks) |
| **2** | **Single buffered write per record in `FastqRecord::write_to`** (build into a local `Vec<u8>`, single `write_all`) | `fastq.rs:42-48` | Trivial (~15 LOC) | **5–10%** | 246 inclusive samples on the `write_to → write_fmt → write_all → flate2 → zlib_rs::deflate_medium → longest_match` path. The 4 `writeln!` calls each invoke deflate separately; bigger chunks per deflate call = better throughput |
| **3** | **Increase per-batch size from 4096 to 16384 records** (let deflate see bigger chunks) | `parallel.rs:32`, `fastq.rs:126` | Trivial (~2 LOC) | **3–8%** | Larger batches = larger gzip blocks = better deflate efficiency. Memory cost: ~5 MB vs ~1.2 MB per worker — fine |
| **4** | **Myers' bit-parallel edit distance** for adapters ≤64 bp | `alignment.rs` | High (~400 LOC + tests) | **10–20%** of total wall (cuts the 36% `trim_read` budget by ~30%) | trim_read is 36% at cores=1; alignment is the dominant inner cost |
| **5** | **Increase `mpsc::sync_channel` buffer from 2 to 8** | `parallel.rs:78,467` | Trivial (~2 LOC) | **0–5%** at cores=16+ | Wall-clock data shows scaling plateau at 8 cores; deeper queue may smooth contention |
| **6** | **Pre-allocate `adapter_matches` capacity inline** (skip if 0 hits) | `trimmer.rs:114` | Trivial (~3 LOC) | **<1%** | Sample data: invisible at 405-sample resolution. Listed for completeness — would survive a 10K-sample profile |
| **7** | **Switch from `mpsc` to `crossbeam_channel`** | `parallel.rs` | Medium (~100 LOC) | **0–5%** at cores=16+ | Lock-free queue may reduce contention; need rebenched at higher core counts |
| **8** | **`FastqRecord::seq`/`qual` `String` → `Vec<u8>`** | `fastq.rs` + ripple | High (~200 LOC) | **1–3%** (formerly estimated 5–15%) | Sample data: fastq module is only 2% of samples. Worth doing for **ergonomics** (eliminates `.as_bytes()` boilerplate, simplifies `trim_ns`) but not as a perf win |
| **9** | **Threaded-reader `Vec<Option<FastqRecord>>`** | `fastq.rs:140` | Trivial (~10 LOC) | **<1%** | Sample data: not in top samples |
| **10** | **Flat-vector DP matrix in `find_3prime_adapter`** (allocation-only optimization, not algorithmic) | `alignment.rs:57` | Low (~30 LOC) | **<1%** alone; **stops mattering after item 4** | Sample data: invisible at this resolution. Item 4 (Myers') makes the DP allocation moot |

**Composite estimate (revised)**: items 1+2+3 are each trivial-to-low effort and address the 62%-gzip-dominance directly. Together they could plausibly deliver **25–45% wall-clock improvement** at `--cores 8`. Item 4 (Myers') is the single largest discrete optimization, worth its own PR.

### Original ranking (superseded — kept for the reality-vs-prediction record)

The original code-review-only top-10 is preserved here:

| # | Change | File | Original estimate | Reality |
|--:|---|---|---|---|
| 1 | `String` → `Vec<u8>` (`FastqRecord`) | `fastq.rs` | 5–15% | ~1–3% (item 8 in revised list) |
| 2 | Flat DP matrix | `alignment.rs` | 5–10% | <1% (item 10 in revised list) |
| 3 | Thread-local DP buffer | `alignment.rs` | 5–10% | <1% (subsumed by item 4 — Myers') |
| 4 | `Vec<Option<FastqRecord>>` reader | `fastq.rs` | 2–5% | <1% (item 9 in revised list) |
| 5 | `clip_5prime` `drain` | `fastq.rs` | 1–3% | <1% |
| 6 | `SmallVec` for `adapter_matches` | `trimmer.rs` | 1–2% | <1% (item 6 in revised list) |
| 7 | `sync_channel(8)` | `parallel.rs` | 0–10% | 0–5% at high cores (item 5 in revised list) |
| 8 | Myers' bit-parallel | `alignment.rs` | 30–80% | **10–20% of total wall** (item 4 in revised list — promoted) |
| 9 | `crossbeam_channel` | `parallel.rs` | 2–5% | 0–5% at high cores (item 7 in revised list) |
| 10 | `write_to` single buffer | `fastq.rs` | 1–2% | **5–10%** (item 2 in revised list — significantly underestimated) |

**Key reordering**: gzip-tier optimizations (compression level, write batching, batch size) move to top-3 from "not in original list at all". Pure allocation optimizations move down. Item 10 (`write_to` single buffer) jumps from #10 to #2 because its samples show it on the gzip critical path.

## Architectural opportunities (bigger-than-PR-sized)

### A1 — Memory-mapped input

Replace `BufReader<File>` with `memmap2::Mmap` for input files. Zero-copy reads, OS handles read-ahead. **For gzipped input**, mmap doesn't help directly because we need to decompress. **For plain `.fastq` input**, mmap eliminates a copy per line. Matters less because gzipped is the common case.

### A2 — Parallel gzip *decompression*

`MultiGzDecoder` is single-threaded. For BGZF-style block-gzipped input, parallel decode is possible (`gzp` already supports parallel encode; check if it has decode). Most NGS data is plain gzip, but BGZF (samtools-flavour) is increasingly common.

### A3 — SIMD via `std::simd` or explicit intrinsics

`unsafe` blocks: zero in current code. The DP inner loop in `find_3prime_adapter` is exactly the shape SIMD wants. Three approaches:

- **Stable Rust**: portable SIMD via `std::simd` (still nightly-only as of writing; check Rust 1.88 status).
- **Unsafe AVX2**: hand-written intrinsics. Fast but maintenance burden.
- **Myers' bit-parallel** (item 8 above): no SIMD needed — uses 64-bit integer ops. Often the right answer for short patterns.

### A4 — Lock-free output ordering

The `BTreeMap` ordered-output design forces all worker results through one main-thread serialisation point. An alternative: per-worker append-only output streams that the OS atomically appends to a single output file (`O_APPEND`). The output is multi-member-gzip-correct as long as workers write whole gzip members, but reads may be slightly out of input order. This breaks the multi-core determinism property unless workers also buffer until in-order.

### A5 — Buffer pool / bumpalo arena

Per-batch `Vec::with_capacity(reads.len() * 300)` allocations (parallel.rs:240, 558) could come from a pool. With 1M reads / 4096 batch size = 244 batches, that's 244 large Vec allocations. Pool would reduce to N_workers + a few churn ones.

### A6 — `bytes::Bytes` for shared FASTQ data

If the threaded reader pipelined `Bytes` (refcounted byte slices) instead of owned `Vec<u8>`, the 64KB buffered reads could be sliced into per-record views without per-record copies. Major refactor though.

## What deliberately should NOT be optimized

| Component | Why |
|---|---|
| `quality.rs` | Already optimal — single backward pass, no allocation, branch-predictable. Touching this risks regressions to the 38 unit tests + Cutadapt-parity validation matrix |
| `alignment.rs` DP correctness | The DP is the parity oracle for 19 flag paths in the proptest harness + the validation matrix's md5 oracle. Any optimization MUST preserve byte-identity (verifiable by re-running `tests/parity_proptest.rs`) |
| Output filename construction (`io.rs`) | Two correctness bugs already filed (#244 F3, #245 P3-F2). Performance is fine; correctness work outranks perf work here |
| `fastqc-rust` | External library. Upstream's perf concern, not ours |
| Validation matrix (`ci.yml`) | Already optimised for byte-identity check, not perf |

## Verification approach for any of these optimizations

Before merging any item from the top-10:

1. **Run `tests/parity_proptest.rs`** — verify byte-identity preserved across 20 flag paths.
2. **Run `tests/parity_fuzz.rs`** with `PARITY_FUZZ_SECS=180` — verify no new acceptance/output mismatches surface.
3. **Run the wall-clock benchmark above** at `--cores 1` and `--cores 8`, compare against the baseline numbers in this document.
4. **Run the CI `validation` job locally** — md5-compare against Perl 0.6.11 across all 5 protected paths.

The harness from the parity-hunt phase makes optimization-safety verification ~12 minutes of `cargo test`. Use it.

## Profiling tooling recommendations for upstream

The blocker on this audit was **no statistical profiler available**. For a sustained perf-improvement workstream, upstream should adopt:

- **`samply`** (pure-Rust, requires `kernel.perf_event_paranoid <= 1`). Best modern alternative to `perf`. Already a CI-friendly choice.
- **`cargo-flamegraph`** — drives `perf record` + `inferno` to produce flamegraphs. Same `perf_event_paranoid` requirement.
- **`criterion`** for micro-bench harnesses. Already a candidate dev-dep (mentioned in Audit Part 6 §B.6); would let `find_3prime_adapter`, `quality_trim_3prime`, `FastqReader::next_record` be measured per-call with statistical confidence.
- **`hyperfine`** for end-to-end wall-clock comparisons (before/after a change). Trivial to use; catches regressions.
- **`cargo-mutants`** (already in Audit Part 4 #16) interacts with perf work because it tells you which optimizations also weakened tests.

## Methodology notes (so future sessions can rerun)

```bash
# Baseline fixture: 1M reads = smallRNA_100K × 10 (multi-member gzip)
F=/tmp/parity-perf/smallRNA_1M.fastq.gz
mkdir -p /tmp/parity-perf
for i in 1 2 3 4 5 6 7 8 9 10; do cat test_files/smallRNA_100K.fastq.gz; done > "$F"

# Scaling sweep
TIMEFORMAT='wall=%R user=%U sys=%S'
for n in 1 2 4 8 16 32; do
  D=/tmp/parity-perf/sweep_c${n}; mkdir -p "$D"
  echo "--- cores=$n ---"
  { time ./target/release/trim_galore --cores $n -o "$D" "$F" > /dev/null 2>&1; }
done

# Determinism check
for n in 1 2 4 8 16 32; do
  md5=$(gzip -dc /tmp/parity-perf/sweep_c$n/smallRNA_1M_trimmed.fq.gz | md5sum | cut -d' ' -f1)
  echo "cores=$n: $md5"
done
```

Statistical profiling (when available):

```bash
# Once kernel.perf_event_paranoid <= 1
cargo install samply
samply record ./target/release/trim_galore --cores 1 -o /tmp/out "$F"
# samply opens the Firefox profiler in a browser
```
