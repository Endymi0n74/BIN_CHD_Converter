//! End-to-end integration test: generate a tiny CD image, convert it to CHD
//! with real `chdman`, then extract it back to `.bin`/`.cue` and verify the
//! outputs. Skipped (not failed) when `chdman` is not available so the
//! suite stays green on machines without MAME (e.g. the app-only CI jobs).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Incrementing counter so concurrent runs / repeated tests don't collide on
/// temp folders.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Total sectors used for the tiny CD image.
const SECTORS: usize = 32;
/// 2352-byte raw MODE1 sectors (sync + address + mode + 2048 data + ECC).
const SECTOR_BYTES: usize = 2352;

fn bin_suffix() -> &'static str {
    if cfg!(windows) { "chdman.exe" } else { "chdman" }
}

/// Finds a runnable `chdman`, checking (in order): the `CHDMAN` env var, the
/// `PATH`, and the binaries committed in `BatchConvertToCHD/` beside the repo.
fn find_chdman() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CHDMAN") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() { return Some(candidate); }
    }
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            let candidate = dir.join(bin_suffix());
            if candidate.is_file() { return Some(candidate); }
        }
    }
    // Fall back to the executables versioned in the repo.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["../../BatchConvertToCHD/chdman", "../../../BatchConvertToCHD/chdman"] {
        let candidate = manifest_dir.join(relative);
        let candidate = candidate.with_extension(if cfg!(windows) { "exe" } else { "" });
        if candidate.is_file() { return Some(candidate); }
    }
    None
}

/// Builds a raw MODE1/2352 sector. The 12-byte sync is all 0xFF; the 3-byte
/// MSF address + mode byte follow, then 2048 bytes of pattern data.
fn build_sector(index: u8) -> [u8; SECTOR_BYTES] {
    let mut sector = [0_u8; SECTOR_BYTES];
    // sync pattern
    sector[0..12].fill(0xFF);
    // 3-byte MSF address (seconds:minutes:frames) then MODE1
    sector[12] = index;
    sector[13] = index;
    sector[14] = index;
    sector[15] = 0x01;
    // 2048 data bytes of a deterministic pattern
    for i in 0..2048 {
        sector[16 + i] = (i as u8).wrapping_mul(index.wrapping_add(1));
    }
    sector
}

/// Writes `name.cue` and `name.bin` describing a single data track.
fn write_cd_image(dir: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let bin_path = dir.join(format!("{name}.bin"));
    let mut bin = Vec::with_capacity(SECTORS * SECTOR_BYTES);
    for sector_index in 0..SECTORS {
        bin.extend_from_slice(&build_sector(sector_index as u8));
    }
    fs::write(&bin_path, &bin).unwrap();
    let cue_path = dir.join(format!("{name}.cue"));
    let cue = format!(
        "FILE \"{name}.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n"
    );
    fs::write(&cue_path, cue).unwrap();
    cue_path
}

fn run_chdman(chdman: &Path, cwd: &Path, args: &[&str]) -> bool {
    let status = Command::new(chdman)
        .current_dir(cwd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to spawn chdman");
    status.success()
}

#[test]
fn chd_convert_and_extract_round_trip() {
    let chdman = match find_chdman() {
        Some(path) => path,
        None => {
            eprintln!(
                "SKIP: chdman not found. Install MAME (or point CHDMAN at chdman) to run this integration test."
            );
            return;
        }
    };

    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    let work = env::temp_dir().join(format!("bin_chd_int_{}_{}", std::process::id(), counter));
    let roms = work.join("roms");
    let output = work.join("output");
    let source = write_cd_image(&roms, "game");

    // 1) Convert the tiny CD (cue/bin) to CHD.
    let chd_path = roms.join("game.chd");
    assert!(
        run_chdman(&chdman, &work, &["createcd", "-i", source.to_str().unwrap(), "-o", chd_path.to_str().unwrap()]),
        "createcd via chdman should succeed for the generated CD image"
    );
    assert!(chd_path.is_file(), "conversion should produce game.chd");

    // 2) Extract the CHD back to a CUE (chdman also emits the same-stem .bin).
    let out_cue = output.join("game-out.cue");
    fs::create_dir_all(&output).unwrap();
    assert!(
        run_chdman(&chdman, &work, &["extractcd", "-i", chd_path.to_str().unwrap(), "-o", out_cue.to_str().unwrap()]),
        "extractcd via chdman should succeed"
    );

    // 3) Verify both the .cue and the .bin were produced and are consistent.
    let out_bin = output.join("game-out.bin");
    assert!(out_cue.is_file(), "extraction should produce a .cue file");
    assert!(out_bin.is_file(), "extraction should produce a same-stem .bin file");

    let source_bin_size = fs::metadata(roms.join("game.bin")).unwrap().len();
    let out_bin_size = fs::metadata(&out_bin).unwrap().len();
    assert_eq!(
        source_bin_size, out_bin_size,
        "extracted .bin size should match the original .bin size"
    );

    let cue_text = fs::read_to_string(&out_cue).unwrap();
    let cue_lower = cue_text.to_ascii_lowercase();
    assert!(cue_lower.contains("file ") && cue_lower.contains("binary"), "extracted .cue should be a BINARY CUE sheet");
    assert!(cue_lower.contains("track 01"), "extracted .cue should contain at least one track");

    // Clean up the temporary workspace.
    let _ = fs::remove_dir_all(&work);
}