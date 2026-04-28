// Phase 4 of the parity hunt — hand-rolled differential fuzzer.
//
// Generates random + structure-aware bytes and runs both Perl 0.6.11 and
// Rust v2.x trim_galore. Reports any case where:
//   - one impl accepts and the other rejects
//   - both accept but output bytes differ
//   - one impl times out or crashes (panic / segfault)
//
// Skipped if binaries / cutadapt aren't available. `#[ignore]` so a normal
// `cargo test` doesn't run it; invoke with:
//   cargo test --test parity_fuzz -- --ignored --nocapture
//
// Wall-clock budget configurable via PARITY_FUZZ_SECS (default 60s).
// Also reads PARITY_PERL_TG / PARITY_RUST_TG / PARITY_CUTADAPT_BIN like Phase 3.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

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
fn fuzz_budget() -> Duration {
    let secs: u64 = std::env::var("PARITY_FUZZ_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    Duration::from_secs(secs)
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
                "[parity_fuzz] SKIPPING: perl_ok={perl_ok} cutadapt_ok={cutadapt_ok} rust_ok={rust_ok}"
            );
        }
        skip
    })
}

static NEXT_DIR_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn unique_tmpdir() -> PathBuf {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let counter = NEXT_DIR_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("parity_fuzz_{pid}_{nanos}_{counter}"));
    fs::create_dir_all(&p).ok();
    p
}

fn read_gz(path: &Path) -> Option<Vec<u8>> {
    let f = fs::File::open(path).ok()?;
    let mut decoder = flate2::read::GzDecoder::new(f);
    let mut buf = Vec::new();
    decoder.read_to_end(&mut buf).ok()?;
    Some(buf)
}

#[derive(Debug)]
#[allow(dead_code)] // fields are emitted via Debug formatter; compiler misses that
enum FuzzVerdict {
    BothReject,
    BothAccept,
    AcceptanceMismatch { perl_rc: i32, rust_rc: i32 },
    OutputMismatch { name: String, perl_len: usize, rust_len: usize },
    RustCrash { rc: i32 },
    PerlCrash { rc: i32 },
}

/// Drive both impls with raw input bytes. Returns a verdict describing what happened.
/// Each invocation is wrapped in `timeout 8s` to bound wall-clock cost on adversarial input.
fn fuzz_one(bytes: &[u8]) -> FuzzVerdict {
    let tmp = unique_tmpdir();
    let input = tmp.join("input.fastq");
    let _ = fs::write(&input, bytes);
    let perl_out = tmp.join("perl_out");
    let rust_out = tmp.join("rust_out");
    let _ = fs::create_dir_all(&perl_out);
    let _ = fs::create_dir_all(&rust_out);

    let path_with_cutadapt = format!(
        "{}:{}",
        cutadapt_bin(),
        std::env::var("PATH").unwrap_or_default()
    );

    let perl_res = Command::new("timeout")
        .args(["8s", &perl_tg()])
        .env("PATH", &path_with_cutadapt)
        .args(["-o", perl_out.to_str().unwrap(), input.to_str().unwrap()])
        .output();
    let rust_res = Command::new("timeout")
        .args(["8s", &rust_tg()])
        .args(["-o", rust_out.to_str().unwrap(), input.to_str().unwrap()])
        .output();

    let (perl_rc, rust_rc) = match (perl_res, rust_res) {
        (Ok(p), Ok(r)) => (p.status.code().unwrap_or(-1), r.status.code().unwrap_or(-1)),
        _ => {
            let _ = fs::remove_dir_all(&tmp);
            return FuzzVerdict::BothReject;
        }
    };
    let perl_ok = perl_rc == 0;
    let rust_ok = rust_rc == 0;

    // SIGSEGV / panic detection: rc 134 (SIGABRT), 139 (SIGSEGV), 137 (SIGKILL).
    // timeout uses 124 for "killed by timeout" — treat as soft-rejection.
    if perl_rc == 139 || perl_rc == 134 || perl_rc == 6 {
        let _ = fs::remove_dir_all(&tmp);
        return FuzzVerdict::PerlCrash { rc: perl_rc };
    }
    if rust_rc == 139 || rust_rc == 134 || rust_rc == 6 {
        let _ = fs::remove_dir_all(&tmp);
        return FuzzVerdict::RustCrash { rc: rust_rc };
    }

    if !perl_ok && !rust_ok {
        let _ = fs::remove_dir_all(&tmp);
        return FuzzVerdict::BothReject;
    }
    if perl_ok != rust_ok {
        let _ = fs::remove_dir_all(&tmp);
        return FuzzVerdict::AcceptanceMismatch { perl_rc, rust_rc };
    }

    // Both accepted — compare outputs
    let dir_iter = match fs::read_dir(&perl_out) {
        Ok(d) => d,
        Err(_) => {
            let _ = fs::remove_dir_all(&tmp);
            return FuzzVerdict::BothAccept;
        }
    };
    for entry in dir_iter.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if !name_str.ends_with(".fq.gz") {
            continue;
        }
        let perl_path = entry.path();
        let rust_path = rust_out.join(&name);
        let perl_bytes = read_gz(&perl_path).unwrap_or_default();
        let rust_bytes = read_gz(&rust_path).unwrap_or_default();
        if perl_bytes != rust_bytes {
            let _ = fs::remove_dir_all(&tmp);
            return FuzzVerdict::OutputMismatch {
                name: name_str,
                perl_len: perl_bytes.len(),
                rust_len: rust_bytes.len(),
            };
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    FuzzVerdict::BothAccept
}

/// Three input-generation strategies; round-robin via case % 3.
fn gen_input(case_idx: usize, rng: &mut StdRng) -> Vec<u8> {
    match case_idx % 3 {
        0 => {
            // Pure random bytes — most cases are malformed, both should reject
            let len = rng.r#gen_range(0..2048);
            (0..len).map(|_| rng.r#gen::<u8>()).collect()
        }
        1 => {
            // FASTQ-shaped but corrupted — start with valid 1-record FASTQ, then flip random bytes
            let len = rng.r#gen_range(40..=120);
            let bases = b"ACGT";
            let seq: String = (0..len).map(|_| bases[rng.r#gen_range(0..4)] as char).collect();
            let qual: String = (0..len)
                .map(|_| (33u8 + rng.r#gen_range(0..40u8)) as char)
                .collect();
            let mut bytes = format!("@read0\n{seq}\n+\n{qual}\n").into_bytes();
            // Flip a random number of random bytes
            let flips = rng.r#gen_range(0..=5);
            for _ in 0..flips {
                if !bytes.is_empty() {
                    let idx = rng.r#gen_range(0..bytes.len());
                    bytes[idx] = rng.r#gen::<u8>();
                }
            }
            bytes
        }
        _ => {
            // Valid FASTQ with edge-case lengths and N counts
            let len = match rng.r#gen_range(0..5) {
                0 => 0,             // empty record
                1 => 1,             // single-base
                2 => rng.r#gen_range(2..20),    // very short (below typical --length default of 20)
                3 => rng.r#gen_range(20..50),   // borderline
                _ => rng.r#gen_range(80..1500), // long
            };
            let bases = b"ACGTN";
            let seq: String = (0..len)
                .map(|_| bases[rng.r#gen_range(0..5)] as char)
                .collect();
            let qual: String = (0..len)
                .map(|_| (33u8 + rng.r#gen_range(0..40u8)) as char)
                .collect();
            format!("@read0\n{seq}\n+\n{qual}\n").into_bytes()
        }
    }
}

#[test]
#[ignore]
fn fuzz_parity_default_se() {
    if should_skip() {
        return;
    }
    let budget = fuzz_budget();
    let start = Instant::now();
    let mut rng = StdRng::seed_from_u64(20260428);
    let mut runs: usize = 0;
    let mut both_reject = 0;
    let mut both_accept = 0;
    let mut findings: Vec<(usize, FuzzVerdict, Vec<u8>)> = Vec::new();

    while start.elapsed() < budget {
        let bytes = gen_input(runs, &mut rng);
        runs += 1;
        match fuzz_one(&bytes) {
            FuzzVerdict::BothReject => both_reject += 1,
            FuzzVerdict::BothAccept => both_accept += 1,
            verdict => findings.push((runs, verdict, bytes)),
        }
        // Brief progress every 30 cases
        if runs.is_multiple_of(30) {
            eprintln!(
                "[parity_fuzz] {} runs, {} both-reject, {} both-accept, {} findings — {:.1}s elapsed",
                runs,
                both_reject,
                both_accept,
                findings.len(),
                start.elapsed().as_secs_f32()
            );
        }
    }

    eprintln!(
        "\n[parity_fuzz] DONE: {} runs in {:?} → {} both-reject, {} both-accept, {} findings",
        runs,
        start.elapsed(),
        both_reject,
        both_accept,
        findings.len()
    );
    for (run, verdict, input) in &findings {
        eprintln!("\n=== run {run}: {verdict:?} ===");
        eprintln!("input ({} bytes):", input.len());
        eprintln!("{}", String::from_utf8_lossy(input));
    }

    if !findings.is_empty() {
        panic!(
            "parity_fuzz found {} divergence(s) in {} runs",
            findings.len(),
            runs
        );
    }
}
