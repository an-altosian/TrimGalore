# 2026-04-28 — Performance audit

| | |
|---|---|
| **Status** | Code-driven audit complete. Per-function review across all 15 source files. Wall-clock baseline established |
| **Scope** | Every function in `src/`, ranked by expected impact. Architectural opportunities identified separately |
| **Methodology** | Wall-clock scaling on 1M-read synthetic fixture (10× smallRNA_100K) + source-code review + cross-cutting allocation/clone grep. **No statistical profiling** — `perf_event_paranoid=2` in this sandbox blocks `samply`/`perf`/`flamegraph`; `valgrind`/`callgrind` not installed. Code review + wall-clock numbers are the substitute |
| **Audience** | Future sessions implementing the optimizations; upstream for review |
| **Related** | [docs/plans/2026-04-28_REVIEW_ci-cd-audit.md](2026-04-28_REVIEW_ci-cd-audit.md), [docs/plans/2026-04-28_FINDINGS_parity-hunt-phase1-2.md](2026-04-28_FINDINGS_parity-hunt-phase1-2.md) |

## Headline findings

1. **The README claim "near-linear speedup up to ~16 cores" overstates** — actual data shows the knee at **8 cores** (3.27× speedup on 1M reads). Beyond 8 cores, wall-clock plateaus and user time grows, indicating contention overhead.
2. **`alignment.rs::find_3prime_adapter` allocates a fresh nested `Vec<Vec<usize>>` per read** — for 1M reads × 1 adapter × ~14×151 cells, this is ~1.4M small allocations on the hottest call site in the codebase. **Highest single-function optimization opportunity.**
3. **`fastq.rs::FastqRecord` uses `String` for sequence/quality** — every line read does `.to_string()` (1 allocation per line × 4 lines × N reads = 4M+ String allocations on 1M reads). Switching to `Vec<u8>` skips UTF-8 validation and enables in-place editing.
4. **No SIMD anywhere in the codebase** — `unsafe` blocks: zero. The DP inner loop in `find_3prime_adapter` is the textbook target for either `std::simd` or Myers' bit-parallel edit distance (10–100× speedup for adapters ≤64bp).
5. **`fastq.rs` threaded reader allocates 3 empty `String`s per record consumption** via `mem::replace` to a fresh `FastqRecord`. Switching the buffer to `Vec<Option<FastqRecord>>` makes this zero-cost via `Option::take`.

## Baseline measurements

### Wall-clock scaling on 1M reads (smallRNA_100K × 10)

```text
cores=1   wall=3.054 user=3.042 sys=0.010   (1.00× baseline)
cores=2   wall=1.944 user=3.853 sys=0.030   (1.57× speedup, 79% efficiency)
cores=4   wall=1.141 user=3.920 sys=0.010   (2.68× speedup, 67% efficiency)
cores=8   wall=0.935 user=4.662 sys=0.020   (3.27× speedup, 41% efficiency) ← KNEE
cores=16  wall=1.025 user=6.191 sys=0.069   (2.98× speedup, 19% efficiency)
cores=32  wall=0.989 user=6.191 sys=0.041   (3.09× speedup, 9.7% efficiency)
```

**Diagnosis**: At 16+ cores, **user time doubles** (3.04s → 6.19s, +103%) without wall-clock improvement. ~3 seconds of CPU is spent on synchronization, allocation, and channel ops at 16 cores. The work-pool can't dispatch fast enough to keep that many workers busy, and gzip-output contention (mpsc channel back to main thread for ordered writing) caps scaling.

### Per-fixture throughput (`--cores 8`)

```text
illumina_10K     (10K reads, 960KB)   wall=0.080  → 125,000 reads/s
BS-seq_10K_R1    (10K reads, 266KB)   wall=0.040  → 250,000 reads/s
nextera_100K     (100K reads, 3.3MB)  wall=0.124  → 806,000 reads/s
smallRNA_100K    (100K reads, 2.0MB)  wall=0.098  → 1,020,000 reads/s
smallRNA_1M      (1M reads, 20MB)     wall=0.935  → 1,070,000 reads/s
```

100K-read inputs are dominated by startup + thread spawn (~50–80ms fixed cost). At 1M reads, sustained throughput is **~1M reads/s** at 8 cores. md5 byte-identity holds across `--cores` 1–32 (the multi-core determinism property from CHANGELOG, verified empirically).

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

## Top-10 highest-impact optimization candidates

Ranked by expected wall-clock impact on the 1M-read benchmark:

| # | Change | File | Effort | Expected gain |
|--:|---|---|---|---|
| **1** | Switch `FastqRecord::seq`/`qual` from `String` to `Vec<u8>` (PERF-2) | `fastq.rs` + ripple in `trimmer.rs`, `quality.rs`, `filters.rs` | High (~200 LOC across files) | 5–15% |
| **2** | Flat `Vec<u8>` DP matrix in `find_3prime_adapter` (PERF-1 quick win) | `alignment.rs` | Low (~30 LOC) | 5–10% |
| **3** | Thread-local DP buffer reuse (PERF-1 better) | `alignment.rs` | Medium (~80 LOC) | 5–10% additional on top of #2 |
| **4** | Threaded-reader `Vec<Option<FastqRecord>>` (PERF-4) | `fastq.rs` | Low (~30 LOC) | 2–5% |
| **5** | `clip_5prime` in-place via `drain` (PERF-3) | `fastq.rs` | Trivial (~5 LOC; depends on PERF-2 for `Vec<u8>`) | 1–3% |
| **6** | `SmallVec<[(usize,usize); 1]>` for `adapter_matches` | `trimmer.rs` | Trivial (~5 LOC + dep) | 1–2% |
| **7** | Increase work-channel buffer to `sync_channel(8)` (PERF-5) | `parallel.rs` | Trivial (~2 LOC) | 0–10% (workload-dependent) |
| **8** | Myers' bit-parallel edit distance for adapters ≤64bp | `alignment.rs` | High (~400 LOC + extensive tests) | 30–80% on adapter-trim phase |
| **9** | Replace `mpsc` with `crossbeam_channel` | `parallel.rs` | Medium (~100 LOC) | 2–5% at 8+ cores |
| **10** | Direct write-to-Vec in `FastqRecord::write_to` (single buffer per record) | `fastq.rs` | Trivial (~10 LOC) | 1–2% |

**Composite estimate**: a session implementing items 1–5 + 7 (the cluster of small-to-medium changes) could plausibly deliver **15–30% wall-clock improvement** on the 1M-read fixture at `--cores 8`. Item 8 alone could approach 30–80% if Myers is feasible — but it's a larger and riskier change because the existing DP has been the parity oracle for 19 flag paths.

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
