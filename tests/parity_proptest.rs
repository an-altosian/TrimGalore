// Phase 3 of the parity hunt — differential property test against Perl 0.6.11.
//
// Generates valid synthetic FASTQ via proptest, runs both Perl and Rust
// trim_galore on the same input, and asserts byte-identical output.
//
// The test is automatically skipped if any of these binaries are not on
// PATH or at the default location; this lets `cargo test` still pass in
// environments without Cutadapt installed.
//
// Override locations via env vars:
//   PARITY_PERL_TG       — path to Perl 0.6.11 trim_galore (default: $TMPDIR/parity-hunt/bin/...)
//   PARITY_RUST_TG       — path to Rust release binary    (default: target/release/trim_galore)
//   PARITY_CUTADAPT_BIN  — directory containing cutadapt  (default: $TMPDIR/parity-hunt/mamba_env_v3/bin)

use flate2::write::GzEncoder;
use flate2::Compression;
use proptest::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const DEFAULT_PERL_TG: &str = "/tmp/claude-470214627/parity-hunt/bin/trim_galore_perl";
const DEFAULT_RUST_TG: &str = "target/release/trim_galore";
const DEFAULT_CUTADAPT_BIN: &str = "/tmp/claude-470214627/parity-hunt/mamba_env_v3/bin";

fn perl_tg() -> String {
    std::env::var("PARITY_PERL_TG").unwrap_or_else(|_| DEFAULT_PERL_TG.into())
}
fn rust_tg() -> String {
    std::env::var("PARITY_RUST_TG").unwrap_or_else(|_| DEFAULT_RUST_TG.into())
}
fn cutadapt_bin() -> String {
    std::env::var("PARITY_CUTADAPT_BIN").unwrap_or_else(|_| DEFAULT_CUTADAPT_BIN.into())
}

static SKIP: OnceLock<bool> = OnceLock::new();

fn should_skip() -> bool {
    *SKIP.get_or_init(|| {
        let perl_ok = Path::new(&perl_tg()).exists();
        let cutadapt_ok = Path::new(&cutadapt_bin()).join("cutadapt").exists();
        let rust_ok = Path::new(&rust_tg()).exists();
        let skip = !(perl_ok && cutadapt_ok && rust_ok);
        if skip {
            eprintln!(
                "[parity_proptest] SKIPPING: perl_ok={perl_ok} cutadapt_ok={cutadapt_ok} rust_ok={rust_ok}"
            );
        }
        skip
    })
}

fn unique_tmpdir() -> PathBuf {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let counter = NEXT_DIR_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("parity_proptest_{pid}_{nanos}_{counter}"));
    fs::create_dir_all(&p).ok();
    p
}
static NEXT_DIR_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn read_gz(path: &Path) -> Vec<u8> {
    let f = fs::File::open(path).expect("open gz");
    let mut decoder = flate2::read::GzDecoder::new(f);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).expect("decode gz");
    buf
}

fn write_gz(path: &Path, content: &str) -> Result<(), String> {
    let f = fs::File::create(path).map_err(|e| format!("create gz: {e}"))?;
    let mut enc = GzEncoder::new(f, Compression::default());
    enc.write_all(content.as_bytes())
        .map_err(|e| format!("gz write: {e}"))?;
    enc.finish().map_err(|e| format!("gz finish: {e}"))?;
    Ok(())
}

/// Run both binaries on `fastq_content` with the given flags. Returns Ok on parity,
/// Err with message on divergence. Either-impl-rejects cases return Ok (treated as
/// out-of-corpus, like prop_assume).
fn run_parity(fastq_content: &str, flags: &[&str]) -> Result<(), String> {
    let tmp = unique_tmpdir();
    // Write gzipped input so both impls produce .fq.gz output (matches the validation
    // matrix setup; routes around P3-F2 where plain .fastq input causes Perl to emit
    // .fq while Rust emits .fq.gz).
    let input = tmp.join("input.fastq.gz");
    write_gz(&input, fastq_content)?;
    let perl_out = tmp.join("perl_out");
    let rust_out = tmp.join("rust_out");
    fs::create_dir_all(&perl_out).map_err(|e| format!("mkdir perl: {e}"))?;
    fs::create_dir_all(&rust_out).map_err(|e| format!("mkdir rust: {e}"))?;

    let path_with_cutadapt = format!(
        "{}:{}",
        cutadapt_bin(),
        std::env::var("PATH").unwrap_or_default()
    );

    let perl_res = Command::new(perl_tg())
        .env("PATH", &path_with_cutadapt)
        .args(flags)
        .args(["-o", perl_out.to_str().unwrap(), input.to_str().unwrap()])
        .output()
        .map_err(|e| format!("perl spawn: {e}"))?;
    let rust_res = Command::new(rust_tg())
        .args(flags)
        .args(["-o", rust_out.to_str().unwrap(), input.to_str().unwrap()])
        .output()
        .map_err(|e| format!("rust spawn: {e}"))?;

    let perl_ok = perl_res.status.success();
    let rust_ok = rust_res.status.success();

    // Both rejected — out of corpus, skip silently
    if !perl_ok && !rust_ok {
        let _ = fs::remove_dir_all(&tmp);
        return Ok(());
    }
    if perl_ok != rust_ok {
        let perl_stderr = String::from_utf8_lossy(&perl_res.stderr).to_string();
        let rust_stderr = String::from_utf8_lossy(&rust_res.stderr).to_string();
        return Err(format!(
            "ACCEPTANCE MISMATCH: perl_rc={} rust_rc={}\nperl stderr (last 200): {}\nrust stderr (last 200): {}\nINPUT:\n{}",
            perl_res.status.code().unwrap_or(-1),
            rust_res.status.code().unwrap_or(-1),
            &perl_stderr.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>(),
            &rust_stderr.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>(),
            fastq_content,
        ));
    }

    // Both succeeded — compare every .fq.gz output Perl produced
    let mut compared = 0;
    for entry in fs::read_dir(&perl_out).map_err(|e| format!("readdir: {e}"))? {
        let entry = entry.map_err(|e| format!("readdir entry: {e}"))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if !name_str.ends_with(".fq.gz") {
            continue;
        }
        let rust_path = rust_out.join(&name);
        if !rust_path.exists() {
            return Err(format!(
                "RUST MISSING OUTPUT: {name_str}\nINPUT:\n{fastq_content}"
            ));
        }
        let perl_bytes = read_gz(&entry.path());
        let rust_bytes = read_gz(&rust_path);
        if perl_bytes != rust_bytes {
            return Err(format!(
                "OUTPUT MISMATCH for {name_str}: perl_decompressed={}b rust_decompressed={}b\nINPUT:\n{fastq_content}\nPERL:\n{}\nRUST:\n{}",
                perl_bytes.len(),
                rust_bytes.len(),
                String::from_utf8_lossy(&perl_bytes),
                String::from_utf8_lossy(&rust_bytes),
            ));
        }
        compared += 1;
    }
    let _ = fs::remove_dir_all(&tmp);
    if compared == 0 {
        return Err(format!(
            "NO OUTPUTS produced by either side\nINPUT:\n{fastq_content}"
        ));
    }
    Ok(())
}

// FASTQ generation strategies

/// Generate (sequence_bytes, quality_bytes) of equal length. Sequence is ACGT only (no N to
/// avoid max_n filter eliminating all reads); quality is Phred+33 in [Q5, Q40], avoiding
/// Q0–Q4 because Cutadapt's --quality 20 default strips everything below Q20 from the 3' end
/// and adversarial Q0 sequences hit a Cutadapt corner case (separately tracked as P3-F1).
fn record_seq_qual() -> impl Strategy<Value = (Vec<u8>, Vec<u8>)> {
    (40usize..=120).prop_flat_map(|len| {
        (
            prop::collection::vec(0u8..4, len..=len),
            prop::collection::vec(38u8..=73, len..=len),
        )
    })
}

/// Build a multi-record FASTQ string from the generated raw byte tuples.
fn fastq_file(min_records: usize, max_records: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(record_seq_qual(), min_records..=max_records).prop_map(|records| {
        let bases = b"ACGT";
        let mut out = String::new();
        for (i, (nucs, quals)) in records.iter().enumerate() {
            let seq: String = nucs.iter().map(|&n| bases[n as usize] as char).collect();
            let qual: String = quals.iter().map(|&q| q as char).collect();
            out.push_str(&format!("@read{i}\n{seq}\n+\n{qual}\n"));
        }
        out
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 50,
        max_shrink_iters: 100,
        ..ProptestConfig::default()
    })]

    /// Default-flag SE parity: same input → same trimmed output, byte-identical.
    /// Cutadapt detects an adapter from random data; both impls must agree on
    /// detected adapter + downstream trim behaviour.
    #[test]
    fn parity_se_default(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        match run_parity(&fastq, &[]) {
            Ok(()) => Ok(()),
            Err(msg) => Err(TestCaseError::fail(msg)),
        }?;
    }
}

// SE-with-flags variants — fewer cases per test since the algorithmic core is
// already stress-tested by parity_se_default; these tests cover flag-dispatch
// surface area, not algorithm correctness.
proptest! {
    #![proptest_config(ProptestConfig {
        cases: 20,
        max_shrink_iters: 50,
        ..ProptestConfig::default()
    })]

    #[test]
    fn parity_se_rrbs(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--rrbs"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_se_small_rna(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--small_rna"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_se_length_50(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--length", "50"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_se_quality_30(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--quality", "30"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_se_hardtrim5_30(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--hardtrim5", "30"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_se_bgiseq(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--bgiseq"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_se_stranded_illumina(fastq in fastq_file(1, 4)) {
        if should_skip() { return Ok(()); }
        run_parity(&fastq, &["--stranded_illumina"]).map_err(TestCaseError::fail)?;
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Paired-end extension
// ──────────────────────────────────────────────────────────────────────────

/// Generate a vector of N records as (seq_bytes, qual_bytes) pairs. Each record
/// will get a shared @readN ID across R1 and R2.
fn paired_records(min_records: usize, max_records: usize)
    -> impl Strategy<Value = Vec<((Vec<u8>, Vec<u8>), (Vec<u8>, Vec<u8>))>>
{
    prop::collection::vec((record_seq_qual(), record_seq_qual()), min_records..=max_records)
}

fn pair_to_strings(pairs: &[((Vec<u8>, Vec<u8>), (Vec<u8>, Vec<u8>))]) -> (String, String) {
    let bases = b"ACGT";
    let mut r1 = String::new();
    let mut r2 = String::new();
    for (i, ((s1, q1), (s2, q2))) in pairs.iter().enumerate() {
        let s1s: String = s1.iter().map(|&n| bases[n as usize] as char).collect();
        let q1s: String = q1.iter().map(|&q| q as char).collect();
        let s2s: String = s2.iter().map(|&n| bases[n as usize] as char).collect();
        let q2s: String = q2.iter().map(|&q| q as char).collect();
        // Trim Galore expects matching read IDs across mates; use the same base ID.
        r1.push_str(&format!("@read{i}/1\n{s1s}\n+\n{q1s}\n"));
        r2.push_str(&format!("@read{i}/2\n{s2s}\n+\n{q2s}\n"));
    }
    (r1, r2)
}

/// Run both binaries on a paired-end input. Returns Ok on parity.
fn run_parity_pe(r1_content: &str, r2_content: &str, flags: &[&str]) -> Result<(), String> {
    let tmp = unique_tmpdir();
    let r1 = tmp.join("r1.fastq.gz");
    let r2 = tmp.join("r2.fastq.gz");
    write_gz(&r1, r1_content)?;
    write_gz(&r2, r2_content)?;
    let perl_out = tmp.join("perl_out");
    let rust_out = tmp.join("rust_out");
    fs::create_dir_all(&perl_out).map_err(|e| format!("mkdir: {e}"))?;
    fs::create_dir_all(&rust_out).map_err(|e| format!("mkdir: {e}"))?;

    let path_with_cutadapt = format!(
        "{}:{}",
        cutadapt_bin(),
        std::env::var("PATH").unwrap_or_default()
    );

    let perl_res = Command::new(perl_tg())
        .env("PATH", &path_with_cutadapt)
        .args(flags)
        .args(["-o", perl_out.to_str().unwrap()])
        .args([r1.to_str().unwrap(), r2.to_str().unwrap()])
        .output()
        .map_err(|e| format!("perl spawn: {e}"))?;
    let rust_res = Command::new(rust_tg())
        .args(flags)
        .args(["-o", rust_out.to_str().unwrap()])
        .args([r1.to_str().unwrap(), r2.to_str().unwrap()])
        .output()
        .map_err(|e| format!("rust spawn: {e}"))?;

    let perl_ok = perl_res.status.success();
    let rust_ok = rust_res.status.success();

    if !perl_ok && !rust_ok {
        let _ = fs::remove_dir_all(&tmp);
        return Ok(());
    }
    if perl_ok != rust_ok {
        let pe = String::from_utf8_lossy(&perl_res.stderr).to_string();
        let re = String::from_utf8_lossy(&rust_res.stderr).to_string();
        return Err(format!(
            "PE ACCEPTANCE MISMATCH flags={flags:?}: perl_rc={} rust_rc={}\nperl: {}\nrust: {}\nR1:\n{r1_content}\nR2:\n{r2_content}",
            perl_res.status.code().unwrap_or(-1),
            rust_res.status.code().unwrap_or(-1),
            &pe.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>(),
            &re.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>(),
        ));
    }

    let mut compared = 0;
    for entry in fs::read_dir(&perl_out).map_err(|e| format!("readdir: {e}"))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if !name_str.ends_with(".fq.gz") {
            continue;
        }
        let rust_path = rust_out.join(&name);
        if !rust_path.exists() {
            return Err(format!(
                "PE RUST MISSING: {name_str} flags={flags:?}\nR1:\n{r1_content}\nR2:\n{r2_content}"
            ));
        }
        let pb = read_gz(&entry.path());
        let rb = read_gz(&rust_path);
        if pb != rb {
            return Err(format!(
                "PE OUTPUT MISMATCH for {name_str} flags={flags:?}: perl={}b rust={}b\nR1:\n{r1_content}\nR2:\n{r2_content}",
                pb.len(), rb.len()
            ));
        }
        compared += 1;
    }
    let _ = fs::remove_dir_all(&tmp);
    if compared == 0 {
        return Err(format!(
            "PE NO OUTPUTS flags={flags:?}\nR1:\n{r1_content}\nR2:\n{r2_content}"
        ));
    }
    Ok(())
}

fn paired_fastq() -> impl Strategy<Value = (String, String)> {
    paired_records(1, 3).prop_map(|pairs| pair_to_strings(&pairs))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 20,
        max_shrink_iters: 50,
        ..ProptestConfig::default()
    })]

    #[test]
    fn parity_pe_default((r1, r2) in paired_fastq()) {
        if should_skip() { return Ok(()); }
        run_parity_pe(&r1, &r2, &["--paired"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_pe_rrbs((r1, r2) in paired_fastq()) {
        if should_skip() { return Ok(()); }
        run_parity_pe(&r1, &r2, &["--paired", "--rrbs"]).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn parity_pe_small_rna((r1, r2) in paired_fastq()) {
        if should_skip() { return Ok(()); }
        run_parity_pe(&r1, &r2, &["--paired", "--small_rna"]).map_err(TestCaseError::fail)?;
    }
}
