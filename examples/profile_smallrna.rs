//! In-process SIGPROF sampling profile of the SE pipeline on the 1M-read fixture.
//!
//! Why this exists: kernel `perf_event_paranoid=2` blocks `samply`/`perf` for
//! unprivileged users in some sandboxes/containers. `pprof-rs` uses POSIX
//! SIGPROF timers and does NOT need kernel perf access — pure userspace.
//!
//! Usage:
//!   cargo run --release --example profile_smallrna
//!
//! Reads from PROFILE_INPUT (default: /tmp/claude-470214627/perf/smallRNA_1M.fastq.gz)
//! and writes the flamegraph SVG to PROFILE_OUT (default: /tmp/claude-470214627/perf/flamegraph_se_cores1.svg).
//! Cores defaults to 1 for cleanest single-threaded profile; override with PROFILE_CORES.

use std::path::PathBuf;
use trim_galore::parallel::{run_paired_end_parallel, run_single_end_parallel};
use trim_galore::trimmer::TrimConfig;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn make_default_config() -> TrimConfig {
    TrimConfig {
        // Default Illumina TruSeq adapter
        adapters: vec![("Illumina".to_string(), b"AGATCGGAAGAGC".to_vec())],
        adapters_r2: vec![],
        times: 1,
        quality_cutoff: 20,
        phred_offset: 33,
        error_rate: 0.1,
        min_overlap: 1,
        length_cutoff: 20,
        max_length: None,
        max_n: None,
        trim_n: false,
        clip_r1: None,
        clip_r2: None,
        three_prime_clip_r1: None,
        three_prime_clip_r2: None,
        rename: false,
        nextseq: false,
        rrbs: false,
        non_directional: false,
        is_paired: false,
        poly_a: false,
        poly_g: false, // disable poly-G for cleanest baseline (no extra trimming pass)
        discard_untrimmed: false,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = PathBuf::from(env_or(
        "PROFILE_INPUT",
        "/tmp/claude-470214627/perf/smallRNA_1M.fastq.gz",
    ));
    let cores: usize = env_or("PROFILE_CORES", "1").parse()?;
    let out_dir = PathBuf::from("/tmp/claude-470214627/perf/profile_run");
    std::fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("smallRNA_1M_trimmed.fq.gz");

    let svg_default = format!(
        "/tmp/claude-470214627/perf/flamegraph_se_cores{}.svg",
        cores
    );
    let svg_path = PathBuf::from(env_or("PROFILE_OUT", &svg_default));

    println!(
        "[profile] input={} cores={} out={} svg={}",
        input.display(),
        cores,
        out_file.display(),
        svg_path.display()
    );

    let config = make_default_config();

    // Sample at 4999 Hz (prime, near pprof-rs safe-max on Linux). Blocklist
    // common stdlib / pthread frames so the resulting flamegraph emphasises
    // trim_galore code. PROFILE_FREQ overrides if a different rate is needed.
    let freq: i32 = env_or("PROFILE_FREQ", "4999").parse()?;
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(freq)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()?;

    let t0 = std::time::Instant::now();

    // --paired support hook (for future use): if PROFILE_R2 is set, run PE.
    let r2 = std::env::var("PROFILE_R2").ok();
    let gzip: bool = env_or("PROFILE_GZIP", "true").parse()?;
    let out_file = if gzip {
        out_file
    } else {
        out_dir.join("smallRNA_1M_trimmed.fq")
    };
    let stats = if let Some(r2_str) = r2 {
        let r2_path = PathBuf::from(r2_str);
        let out_r1 = out_dir.join("paired_r1_val_1.fq.gz");
        let out_r2 = out_dir.join("paired_r2_val_2.fq.gz");
        let mut pe_config = config;
        pe_config.is_paired = true;
        let (s1, _s2, _ps) = run_paired_end_parallel(
            &input,
            &r2_path,
            &out_r1,
            &out_r2,
            None,
            None,
            &pe_config,
            cores,
            true, // gzip
            trim_galore::filters::UnpairedLengths { r1: 35, r2: 35 },
        )?;
        s1
    } else {
        run_single_end_parallel(&input, &out_file, &config, cores, gzip)?
    };

    let elapsed = t0.elapsed();
    println!(
        "[profile] processed {} reads in {:.2}s ({:.0} reads/s)",
        stats.total_reads,
        elapsed.as_secs_f64(),
        stats.total_reads as f64 / elapsed.as_secs_f64()
    );

    // Build the report and emit a flamegraph SVG.
    let report = guard.report().build()?;
    let file = std::fs::File::create(&svg_path)?;
    report.flamegraph(file)?;
    println!("[profile] wrote flamegraph SVG: {}", svg_path.display());

    // Also write a folded-stack text file for grep-friendly analysis.
    let folded_path = svg_path.with_extension("folded.txt");
    let mut folded = std::fs::File::create(&folded_path)?;
    use std::io::Write;
    for (frames, count) in &report.data {
        let frame_str: Vec<String> = frames
            .frames
            .iter()
            .rev()
            .flat_map(|f| f.iter())
            .map(|s| s.name())
            .collect();
        writeln!(folded, "{} {}", frame_str.join(";"), count)?;
    }
    println!("[profile] wrote folded stacks: {}", folded_path.display());
    Ok(())
}
