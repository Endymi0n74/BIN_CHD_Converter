#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};
use tempfile::TempDir;
use walkdir::WalkDir;

/// Shared cancellation state for in-flight batches. `requested` is set by the
/// Cancel button; `running` holds the chdman child so the cancel command can
/// kill it immediately.
#[derive(Default)]
struct Cancellation {
    requested: AtomicBool,
    running: Mutex<Option<Child>>,
}

/// Outcome of running one chdman file operation, distinguishing a clean
/// process failure from a user-initiated cancellation so logs stay accurate.
#[derive(Clone, Copy, PartialEq)]
enum CommandStatus {
    Ok,
    Failed,
    Canceled,
}

#[tauri::command]
fn cancel_batch(state: State<'_, Cancellation>) -> bool {
    state.requested.store(true, Ordering::SeqCst);
    let was_running = if let Some(mut child) = state.running.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
        true
    } else {
        false
    };
    was_running
}

fn find_command(names: &[&str]) -> Option<PathBuf> {
    let mut roots = vec![env::current_exe().ok()?.parent()?.to_path_buf()];
    roots.extend(env::var_os("PATH").as_deref().map(env::split_paths).into_iter().flatten());
    for root in roots {
        for name in names {
            let file = if cfg!(windows) && !name.ends_with(".exe") { format!("{name}.exe") } else { (*name).into() };
            let candidate = root.join(file);
            if candidate.is_file() { return Some(candidate); }
        }
    }
    None
}

/// Longest chdman handles reliably. Its CRT file APIs use ANSI paths capped at
/// MAX_PATH (260); longer input/output paths fail with "No such file or
/// directory" even when the file exists. Mirrors PathUtils.MaxChdmanPath.
const MAX_CHDMAN_PATH: usize = 260;

/// True when every character of `path` is ASCII (`<= 127`). chdman converts its
/// UTF-16 command line down to the ANSI code page, so non-ASCII paths (accented
/// user names, non-Latin folder names) can be mangled and fail with
/// "No such file or directory".
fn is_ascii_path(path: &Path) -> bool {
    path.to_string_lossy().chars().all(|c| c as u32 <= 127)
}

/// True when a path is safe to hand to chdman as-is: pure ASCII and below
/// [`MAX_CHDMAN_PATH`].
fn is_chdman_safe_path(path: &Path) -> bool {
    path.as_os_str().len() < MAX_CHDMAN_PATH && is_ascii_path(path)
}

/// Fixed disk roots to try as ASCII-safe temp bases, best first. On Windows these
/// are the drive roots; elsewhere the filesystem root.
fn fixed_drive_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        for letter in b'C'..=b'Z' {
            roots.push(PathBuf::from(format!("{}:\\", letter as char)));
        }
    }
    #[cfg(not(windows))]
    {
        roots.push(PathBuf::from("/"));
    }
    roots
}

/// Creates a unique temporary directory whose full path is pure ASCII and well below
/// MAX_PATH, for staging inputs/outputs that cannot be handed to chdman directly (paths
/// containing non-ASCII characters or too long). The system temp dir is preferred when it
/// is itself chdman-safe; otherwise an ASCII-named folder on a fixed drive root is used.
/// Mirrors PathUtils.CreateAsciiSafeTempDirectory.
fn create_ascii_safe_temp_dir(prefix: &str) -> Result<TempDir, String> {
    let system_temp = env::temp_dir();
    if is_chdman_safe_path(&system_temp) {
        if let Ok(dir) = tempfile::Builder::new().prefix(prefix).tempdir_in(&system_temp) {
            return Ok(dir);
        }
    }
    for root in fixed_drive_roots() {
        if let Ok(dir) = tempfile::Builder::new().prefix(prefix).tempdir_in(&root) {
            return Ok(dir);
        }
    }
    // Best effort: keep the operation alive rather than failing outright.
    tempfile::Builder::new().prefix(prefix).tempdir().map_err(|e| e.to_string())
}

/// Deterministic hex string used to disambiguate staged names.
fn hex_suffix(input: &Path) -> String {
    let digest = input.to_string_lossy();
    let mut value = 0u64;
    for byte in digest.as_bytes().iter().take(16) { value = value.wrapping_mul(31).wrapping_add(*byte as u64); }
    format!("{value:016x}")
}

#[tauri::command]
fn chdman_status() -> bool { find_command(&["chdman"]).is_some() }

#[derive(Serialize)]
struct DependencyStatus { chdman: bool, seven_zip: bool, maxcso: bool }

#[tauri::command]
fn dependency_status() -> DependencyStatus {
    DependencyStatus {
        chdman: chdman_status(),
        seven_zip: find_command(&["7zz", "7z", "7za"]).is_some(),
        maxcso: find_command(&["maxcso"]).is_some(),
    }
}

fn extensions(mode: &str) -> &'static [&'static str] {
    if mode == "convert" { &["cue", "gdi", "toc", "iso", "img", "raw", "cso", "ciso", "zip", "7z", "rar", "pbp", "ccd", "ecm", "mds"] } else { &["chd"] }
}

#[tauri::command]
fn scan_files(folder: String, mode: String, recursive: bool) -> Result<Vec<String>, String> {
    let depth = if recursive { usize::MAX } else { 1 };
    let mut files: Vec<_> = WalkDir::new(folder).max_depth(depth).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension().and_then(|x| x.to_str()).is_some_and(|x| extensions(&mode).contains(&x.to_ascii_lowercase().as_str())))
        .map(|entry| entry.path().display().to_string()).collect();
    files.sort();
    Ok(files)
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() { return path; }
    let dir = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default();
    for number in 1.. {
        let candidate = dir.join(format!("{stem} ({number}){ext}"));
        if !candidate.exists() { return candidate; }
    }
    unreachable!()
}

/// True when `target` (the primary extraction output) or its chdman sidecar already
/// exists, so writing there would destroy existing files. Mirrors the fork's
/// fd338da behaviour of diverting extraction before clobbering anything.
fn collides_with_output(target: &Path, cmd: &str) -> bool {
    if target.exists() { return true; }
    // chdman extractcd writes a same-stem .bin beside the .cue; treat it as occupied too,
    // so a stray `game.bin` forces the disc into its own subfolder rather than adding a cue next to it.
    cmd == "extractcd" && target.with_extension("bin").exists()
}

/// Returns a free subdirectory under `parent` named after `base` ("name", "name (2)", …)
/// without creating it, mirroring the fork's ReserveFreeSubdirectory.
fn reserve_free_directory(parent: &Path, base: &str) -> PathBuf {
    let first = parent.join(base);
    if !first.exists() { return first; }
    for number in 2..=999 {
        let candidate = parent.join(format!("{base} ({number})"));
        if !candidate.exists() { return candidate; }
    }
    // Bounded search exhausted; fall back to an un-collidable name.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    parent.join(format!("{base}_{nanos:x}"))
}

fn command_for(path: &Path, mode: &str) -> (&'static str, &'static str) {
    if mode == "verify" { return ("verify", ""); }
    match path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "cue" | "gdi" | "toc" => ("createcd", "chd"),
        "iso" => ("createdvd", "chd"),
        "raw" => ("createraw", "chd"),
        _ => ("createhd", "chd"),
    }
}

/// The 12-byte sync pattern every raw CD sector opens with: 00 followed by ten FF and a 00.
/// Mirrors RawCdImageDetector.SyncMark.
const CD_SYNC_MARK: [u8; 12] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Bytes per sector in a raw CD image (2048 user bytes plus sync, header and EDC/ECC).
const RAW_CD_SECTOR_SIZE: u64 = 2352;

/// Offset of the mode byte: 12 bytes of sync plus a 3-byte MSF address.
const RAW_CD_MODE_OFFSET: usize = 15;

/// Extensions under which raw CD dumps get mislabelled, so worth sniffing before trusting the
/// extension (mirrors RawCdImageDetector.CandidateExtensions).
fn is_raw_cd_candidate(ext: &str) -> bool {
    matches!(ext, "iso" | "img" | "bin")
}

/// Reads up to 16 leading bytes of `path`. Returns `None` on any I/O error.
fn read_header(path: &Path) -> Option<[u8; 16]> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    use std::io::Read;
    let mut header = [0_u8; 16];
    let mut filled = 0;
    while filled < header.len() {
        let read = reader.read(&mut header[filled..]).ok()?;
        if read == 0 { break; }
        filled += read;
    }
    Some(header)
}

/// True when `path` is a raw CD image (2352-byte sectors with the sync mark) and returns the
/// track-mode byte (1 = MODE1/2352, 2 = MODE2/2352). `None` when the size is not a whole number
/// of sectors, the sync mark is absent, or any read fails. Mirrors RawCdImageDetector.DetectTrackMode.
fn probe_raw_cd(path: &Path) -> Option<u8> {
    let length = std::fs::metadata(path).ok()?.len();
    // Every raw CD image is a whole number of 2352-byte sectors.
    if length == 0 || length % RAW_CD_SECTOR_SIZE != 0 { return None; }
    let header = read_header(path)?;
    classify_raw_cd_header(&header)
}

/// A byte-for-byte equivalent of the raw CD detection test, decoupled from the filesystem so it
/// can be unit-tested against synthetic sector headers.
fn classify_raw_cd_header(header: &[u8]) -> Option<u8> {
    if header.len() <= RAW_CD_MODE_OFFSET { return None; }
    if header[..CD_SYNC_MARK.len()] != CD_SYNC_MARK { return None; }
    match header[RAW_CD_MODE_OFFSET] {
        1 | 2 => Some(header[RAW_CD_MODE_OFFSET]),
        _ => None,
    }
}

/// Writes a single-track cue for `image_path` into `dir` and returns the cue path. The cue
/// references the image by absolute path so chdman can open the original file wherever it is.
/// Mirrors the single-track cue that BinCueGenerator/RawCdImageDetector produce for raw CD bins.
fn write_raw_cd_cue(dir: &Path, image_path: &Path, mode: u8) -> Result<PathBuf, String> {
    let track = if mode == 1 { "MODE1/2352" } else { "MODE2/2352" };
    let cue_path = dir.join(format!("{}.cue", image_path.file_stem().unwrap_or_default().to_string_lossy()));
    let file_name = image_path.file_name().unwrap_or_default().to_string_lossy();
    let content = format!("FILE \"{}\" BINARY\n  TRACK 01 {track}\n    INDEX 01 00:00:00\n", file_name);
    std::fs::write(&cue_path, content).map_err(|e| e.to_string())?;
    Ok(cue_path)
}

/// Standard CD/DVD sector sizes used to validate disc image alignment. 2448 and 2368 are raw CD
/// sectors carrying subchannel data (2352+96 and 2352+16), which Alcohol and CloneCD rips use.
/// Mirrors IsoSectorValidator.StandardSectorSizes.
const STANDARD_SECTOR_SIZES: [u64; 6] = [2352, 2048, 2336, 2324, 2448, 2368];

/// True when `len` is a whole multiple of any standard sector size (and non-zero).
fn is_sector_aligned(len: u64) -> bool {
    len > 0 && STANDARD_SECTOR_SIZES.iter().any(|size| len % size == 0)
}

/// Returns a warning when `path` has a size not divisible by any standard sector size, or None when
/// aligned, when it is a descriptor file (.cue/.gdi/.toc, whose size is irrelevant), or when the
/// size cannot be read. Mirrors IsoSectorValidator.GetSectorSizeWarning.
fn sector_size_warning(path: &Path) -> Option<String> {
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    if matches!(ext.as_str(), "cue" | "gdi" | "toc") { return None; }
    let len = std::fs::metadata(path).ok()?.len();
    if len > 0 && !is_sector_aligned(len) {
        return Some(format!(
            "file size ({len} bytes) is not divisible by any standard sector size (2048/2324/2336/2352/2368/2448). The file may be corrupt or truncated."
        ));
    }
    None
}

fn explicit_extract_format(format: &str) -> Option<(&'static str, &'static str)> {
    // Explicit user choice: "cue" forces BIN+CUE output (extractcd writes both), "iso" forces DVD, "img" forces HDD.
    match format {
        "cue" | "bin" => Some(("extractcd", "cue")),
        "iso" => Some(("extractdvd", "iso")),
        "img" => Some(("extracthd", "img")),
        _ => None,
    }
}

fn detect_extract_command(info_text: &str) -> (&'static str, &'static str) {
    let text = info_text.to_ascii_lowercase();
    if text.contains("dvd") { ("extractdvd", "iso") }
    // chdman labels hard disks with the 'GDDD' metadata tag.
    else if text.contains("gddd") || text.contains("hard disk") || text.contains("hdd") { ("extracthd", "img") }
    // CD-ROM (CHT2) and GD-ROM (GDROM) both extract to BIN+CUE via chdman extractcd.
    else { ("extractcd", "cue") }
}

fn extraction_command(chdman: &Path, input: &Path, format: &str) -> (&'static str, &'static str) {
    if let Some(choice) = explicit_extract_format(format) { return choice; }
    let info = Command::new(chdman).arg("info").arg("-i").arg(input).output();
    let text = info.map(|value| format!("{}\n{}", String::from_utf8_lossy(&value.stdout), String::from_utf8_lossy(&value.stderr))).unwrap_or_default();
    detect_extract_command(&text)
}

fn prepared_inputs(input: &Path, mode: &str) -> Result<(Vec<PathBuf>, Option<TempDir>), String> {
    let ext = input.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "pbp" || ext == "ccd" {
        let helper = find_command(&["batch-format-helper"]).ok_or("PBP/CCD support requires batch-format-helper beside the app")?;
        let temp = TempDir::new().map_err(|e| e.to_string())?;
        let result = Command::new(helper).arg(&ext).arg(input).arg(temp.path()).output().map_err(|e| e.to_string())?;
        if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into_owned()); }
        return Ok((String::from_utf8_lossy(&result.stdout).lines().map(PathBuf::from).collect(), Some(temp)));
    }
    if ext == "cso" || ext == "ciso" {
        let temp = TempDir::new().map_err(|e| e.to_string())?;
        if let Some(helper) = find_command(&["batch-format-helper"]) {
            let result = Command::new(helper).arg("cso").arg(input).arg(temp.path()).output().map_err(|e| e.to_string())?;
            if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into_owned()); }
            return Ok((String::from_utf8_lossy(&result.stdout).lines().map(PathBuf::from).collect(), Some(temp)));
        }
        let tool = find_command(&["maxcso"]).ok_or("CSO support requires batch-format-helper or maxcso")?;
        let iso = temp.path().join(format!("{}.iso", input.file_stem().unwrap_or_default().to_string_lossy()));
        let result = Command::new(tool).arg("--decompress").arg(input).arg("-o").arg(&iso).output().map_err(|e| e.to_string())?;
        if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into_owned()); }
        return Ok((vec![iso], Some(temp)));
    }
    if ext == "ecm" || ext == "mds" {
        // ECM decodes to an image; MDS prepares a cue or DVD image. Both route through the sidecar,
        // which prints the file(s) chdman should receive next.
        let helper = find_command(&["batch-format-helper"]).ok_or("ECM/MDS support requires batch-format-helper beside the app")?;
        let temp = TempDir::new().map_err(|e| e.to_string())?;
        let result = Command::new(helper).arg(&ext).arg(input).arg(temp.path()).output().map_err(|e| e.to_string())?;
        if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into_owned()); }
        let mut prepared: Vec<PathBuf> = String::from_utf8_lossy(&result.stdout).lines().map(PathBuf::from).collect();
        // An ECM-decoded image is often a raw CD dump saved as .bin/.img; route it to createcd by
        // generating a cue, exactly like the content-based routing for native raw-CD files.
        if ext == "ecm" {
            let mut rerouted = Vec::new();
            for path in prepared {
                if let Some(track_mode) = probe_raw_cd(&path) {
                    rerouted.push(write_raw_cd_cue(temp.path(), &path, track_mode)?);
                } else if !rerouted.contains(&path) {
                    rerouted.push(path);
                }
            }
            prepared = rerouted;
        }
        return Ok((prepared, Some(temp)));
    }
    if ["zip", "7z", "rar"].contains(&ext.as_str()) {
        let tool = find_command(&["7zz", "7z", "7za"]).ok_or("Archive support requires 7zz/7z in PATH or beside the app")?;
        let temp = TempDir::new().map_err(|e| e.to_string())?;
        let result = Command::new(tool).arg("x").arg(input).arg(format!("-o{}", temp.path().display())).arg("-y").output().map_err(|e| e.to_string())?;
        if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into_owned()); }
        let files = WalkDir::new(temp.path()).into_iter().filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().and_then(|x| x.to_str()).is_some_and(|x| ["cue", "gdi", "toc", "iso", "img", "raw"].contains(&x.to_ascii_lowercase().as_str())))
            .map(|entry| entry.into_path()).collect();
        return Ok((files, Some(temp)));
    }
    // Content-based routing: a `.bin`/`.img`/`.iso` that is actually a raw 2352-byte-per-sector CD
    // dump would otherwise go to createdvd/createhd (chosen by extension) and fail on the sector
    // arithmetic. Generate a cue so it converts as the CD it is. Mirrors DiscImageSignature /
    // RawCdImageDetector / BinCueGenerator of the fork.
    if mode == "convert" && is_raw_cd_candidate(&ext) {
        if let Some(track_mode) = probe_raw_cd(input) {
            let temp = TempDir::new().map_err(|e| e.to_string())?;
            let cue = write_raw_cd_cue(temp.path(), input, track_mode)?;
            return Ok((vec![cue], Some(temp)));
        }
    }
    Ok((vec![input.to_path_buf()], None))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    kind: String,
    message: String,
    file: String,
    file_index: usize,
    total_files: usize,
    file_percent: f64,
    overall_percent: f64,
    elapsed_seconds: u64,
    remaining_seconds: Option<u64>,
}

fn parse_percent(text: &str) -> Option<f64> {
    for token in text.split_whitespace() {
        let clean = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '%');
        if let Some(number) = clean.strip_suffix('%') {
            if let Ok(value) = number.parse::<f64>() { return Some(value.clamp(0.0, 100.0)); }
        }
    }
    None
}

fn emit_progress(app: &AppHandle, kind: &str, message: &str, file: &Path, index: usize, total: usize, file_percent: f64, started: Instant) {
    let overall = if total == 0 { 100.0 } else { ((index as f64 + file_percent / 100.0) / total as f64 * 100.0).clamp(0.0, 100.0) };
    let elapsed = started.elapsed().as_secs();
    let remaining = if overall > 0.1 && overall < 100.0 { Some(((elapsed as f64 * (100.0 - overall) / overall).round()) as u64) } else { None };
    let _ = app.emit("batch-progress", ProgressEvent {
        kind: kind.into(), message: message.into(), file: file.display().to_string(), file_index: index + 1,
        total_files: total, file_percent, overall_percent: overall, elapsed_seconds: elapsed, remaining_seconds: remaining,
    });
}

fn stream_reader<R: Read + Send + 'static>(mut reader: R, sender: mpsc::Sender<String>) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 1024];
        let mut line = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    for byte in &buffer[..count] {
                        if *byte == b'\r' || *byte == b'\n' {
                            let text = String::from_utf8_lossy(&line).trim().to_string();
                            if !text.is_empty() { let _ = sender.send(text); }
                            line.clear();
                        } else {
                            line.push(*byte);
                        }
                    }
                }
            }
        }
        let text = String::from_utf8_lossy(&line).trim().to_string();
        if !text.is_empty() { let _ = sender.send(text); }
    });
}

/// Runs a command, streaming its output to the UI. Returns `None` when the run
/// was canceled (the child is killed and partial output is cleaned by the
/// caller), `Some(true)` on success and `Some(false)` on a normal failure.
fn run_streamed(
    app: &AppHandle,
    state: &State<'_, Cancellation>,
    command: &mut Command,
    input: &Path,
    index: usize,
    total: usize,
    started: Instant,
) -> Result<Option<bool>, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let shown = format!("$ {:?}", command);
    emit_progress(app, "command", &shown, input, index, total, 0.0, started);
    let child = command.spawn().map_err(|e| e.to_string())?;
    *state.running.lock().unwrap() = Some(child);
    let (sender, receiver) = mpsc::channel();
    {
        let mut guard = state.running.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            if let Some(stdout) = child.stdout.take() { stream_reader(stdout, sender.clone()); }
            if let Some(stderr) = child.stderr.take() { stream_reader(stderr, sender.clone()); }
        }
    }
    drop(sender);
    let mut last_percent = 0.0;
    loop {
        // Honor a user cancellation request as soon as possible.
        if state.requested.load(Ordering::SeqCst) {
            let mut guard = state.running.lock().unwrap();
            if let Some(mut child) = guard.take() {
                emit_progress(app, "output", "Interrupting chdman…", input, index, total, last_percent, started);
                let _ = child.kill();
                let _ = child.wait();
            }
            return Ok(None);
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if let Some(percent) = parse_percent(&line) { last_percent = percent; }
                emit_progress(app, if parse_percent(&line).is_some() { "progress" } else { "output" }, &line, input, index, total, last_percent, started);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut guard = state.running.lock().unwrap();
                if let Some(child) = guard.as_mut() {
                    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                        let successful = status.success();
                        drop(guard);
                        *state.running.lock().unwrap() = None;
                        while let Ok(line) = receiver.try_recv() { emit_progress(app, "output", &line, input, index, total, last_percent, started); }
                        return Ok(Some(successful));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let result = {
                    let mut guard = state.running.lock().unwrap();
                    guard.take().map(|mut child| child.wait().map(|status| status.success()).map_err(|e| e.to_string()))
                };
                match result {
                    Some(outcome) => return outcome.map(Some).map_err(|e| e),
                    None => return Ok(None),
                }
            }
        }
    }
}

fn remove_partial_output(out: &Path, cmd: &str) {
    let _ = fs::remove_file(out);
    // chdman extractcd writes both the .cue and a same-stem .bin; remove the sidecar too.
    if cmd == "extractcd" { let _ = fs::remove_file(out.with_extension("bin")); }
}

fn run_single(
    app: &AppHandle,
    state: &State<'_, Cancellation>,
    chdman: &Path,
    input: &Path,
    source: &str,
    output: &str,
    mode: &str,
    format: &str,
    index: usize,
    total: usize,
    started: Instant,
) -> Result<CommandStatus, String> {
    // bdd2531: keep chdman away from non-ASCII or MAX_PATH-../oversized paths. Whenever the
    // input, or the output root, cannot be handed to chdman as-is, stage the disc in a short
    // ASCII-safe temp dir and deploy the produced files back to the real destination afterwards.
    // Only the OS, never chdman, has to cope with those paths. Never stage in verify mode (no
    // output is produced).
    let staging: Option<TempDir> = if mode != "verify" && (!is_chdman_safe_path(input) || !is_chdman_safe_path(Path::new(output))) {
        Some(create_ascii_safe_temp_dir("bchd_")?)
    } else {
        None
    };
    let staged_input = staging.as_ref().map(|dir| dir.path().join(format!("input_{}.chd", hex_suffix(input))));
    let working_input = match &staged_input {
        Some(path) => {
            fs::copy(input, path).map_err(|e| format!("staging input: {e}"))?;
            path.clone()
        }
        None => input.to_path_buf(),
    };

    // Auto-detection runs chdman `info` too, so it must see the staged (ASCII) input.
    let (cmd, ext) = if mode == "extract" { extraction_command(chdman, &working_input, format) } else { command_for(&working_input, mode) };
    let mut command = Command::new(chdman);
    command.arg(cmd).arg("-i").arg(&working_input);
    let out = if mode == "verify" { None } else {
        let rel = if input.starts_with(source) { input.strip_prefix(source).unwrap_or(input) } else { Path::new(input.file_name().unwrap_or_default()) };
        let target = Path::new(output).join(rel).with_extension(ext);
        // fd338da: on extraction, never overwrite existing files. If the primary output
        // (or its .bin sidecar, for extractcd) is already occupied, divert the whole disc
        // into a free subfolder named after it, keeping the relative cue/bin pairing valid.
        let destination = if mode == "extract" && collides_with_output(&target, cmd) {
            let base = rel.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty())
                .unwrap_or_else(|| "extracted".into());
            let dir = reserve_free_directory(Path::new(output), &base);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            dir.join(format!("{base}.{ext}"))
        } else {
            target
        };
        if let Some(parent) = destination.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let unique = unique_path(destination);
        // chdman writes the output through `-o`; if we are staging anyway, aim the output at the
        // staging dir so chdman never sees a non-ASCII/overlong path, then deploy below.
        let effective_out = match &staging {
            Some(dir) => dir.path().join(format!("output_{}.{ext}", hex_suffix(input))),
            None => unique.clone(),
        };
        command.arg("-o").arg(&effective_out);
        Some((unique, effective_out))
    };
    let ok = run_streamed(app, state, &mut command, input, index, total, started)?;
    match ok {
        Some(true) => {
            // bdd2531: on success with a staging dir, copy the produced files back to the real
            // destination (cue and, for extractcd, its same-stem .bin).
            if let (Some(dir), Some((final_path, staged_path))) = (&staging, &out) {
                deploy_staged_outputs(dir, staged_path, final_path, cmd)?;
            }
            Ok(CommandStatus::Ok)
        }
        Some(false) => {
            // Deploy nothing on failure; clean up any partial at the destination.
            if let Some((final_path, _)) = &out { remove_partial_output(final_path, cmd); }
            Ok(CommandStatus::Failed)
        }
        None => {
            if let Some((final_path, _)) = &out { remove_partial_output(final_path, cmd); }
            Ok(CommandStatus::Canceled)
        }
    }
}

/// Copies the produced staging outputs back to the real destination. Mirrors the fork's deploy of
/// work-dir contents back to the target folder; only called on success when staging was needed.
fn deploy_staged_outputs(_staging: &TempDir, from: &Path, to: &Path, cmd: &str) -> Result<(), String> {
    if let Some(parent) = to.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    fs::copy(from, to).map_err(|e| format!("deploy {to:?}: {e}"))?;
    // extractcd also produced a same-stem `.bin` beside the staged `.cue`.
    if cmd == "extractcd" {
        let staged_bin = from.with_extension("bin");
        let final_bin = to.with_extension("bin");
        if staged_bin.is_file() { fs::copy(&staged_bin, &final_bin).map_err(|e| format!("deploy {final_bin:?}: {e}"))?; }
    }
    Ok(())
}

fn process_one(
    app: &AppHandle,
    state: &State<'_, Cancellation>,
    chdman: &Path,
    original: &Path,
    source: &str,
    output: &str,
    mode: &str,
    format: &str,
    delete_source: bool,
    index: usize,
    total: usize,
    started: Instant,
) -> Vec<String> {
    let (inputs, _temp) = match prepared_inputs(original, mode) {
        Ok(value) => value,
        Err(error) => {
            let message = format!("FAILED: {} ({})", original.display(), error);
            emit_progress(app, "error", &message, original, index, total, 100.0, started);
            return vec![message];
        }
    };
    let mut logs = Vec::new();
    let mut all_ok = true;
    let mut canceled = false;
    for input in inputs {
        // IsoSectorValidator preflight: skip obviously corrupt/truncated images before spending a
        // long chdman run that will fail anyway. Only makes sense for conversion targets, not for
        // CHD extraction (a .chd's compressed size is never sector-aligned).
        if mode == "convert" {
            if let Some(warning) = sector_size_warning(&input) {
                all_ok = false;
                let message = format!("SKIP: {} ({})", input.display(), warning);
                logs.push(message.clone());
                emit_progress(app, "error", &message, &input, index, total, 100.0, started);
                continue;
            }
        }
        match run_single(app, state, chdman, &input, source, output, mode, format, index, total, started) {
            Ok(CommandStatus::Ok) => {
                let message = format!("OK: {}", input.display());
                logs.push(message.clone());
                emit_progress(app, "success", &message, &input, index, total, 100.0, started);
            }
            Ok(CommandStatus::Canceled) => {
                canceled = true;
                let message = format!("CANCELED: {}", input.display());
                logs.push(message.clone());
                emit_progress(app, "canceled", &message, &input, index, total, 100.0, started);
                break;
            }
            result => {
                all_ok = false;
                let detail = match result { Err(error) => format!(" ({})", error), _ => String::new() };
                let message = format!("FAILED: {}{}", input.display(), detail);
                logs.push(message.clone());
                emit_progress(app, "error", &message, &input, index, total, 100.0, started);
            }
        }
    }
    if all_ok && !canceled && delete_source {
        if let Err(error) = fs::remove_file(original) {
            let message = format!("WARN: could not delete {} ({})", original.display(), error);
            logs.push(message.clone());
            emit_progress(app, "output", &message, original, index, total, 100.0, started);
        }
    }
    logs
}
#[tauri::command]
fn process_batch(
    app: AppHandle,
    state: State<'_, Cancellation>,
    source: String,
    output: String,
    mode: String,
    recursive: bool,
    delete_source: bool,
    extract_format: Option<String>,
) -> Result<Vec<String>, String> {
    let chdman = find_command(&["chdman"]).ok_or("chdman was not found. Install MAME first.")?;
    let files = scan_files(source.clone(), mode.clone(), recursive)?;
    let total = files.len();
    let started = Instant::now();
    let format = extract_format.unwrap_or_default();
    // Reset any leftover cancel flag so a fresh batch is not immediately killed.
    state.requested.store(false, Ordering::SeqCst);
    let mut logs = Vec::new();
    for (index, item) in files.into_iter().enumerate() {
        let original = PathBuf::from(&item);
        emit_progress(&app, "start", "Preparing input", &original, index, total, 0.0, started);
        logs.extend(process_one(&app, &state, &chdman, &original, &source, &output, &mode, &format, delete_source, index, total, started));
        if logs.iter().any(|line| line.starts_with("CANCELED:")) { break; }
    }
    let message = if logs.iter().any(|line| line.starts_with("CANCELED:")) { "Batch canceled" } else { "Batch complete" };
    let kind = if message == "Batch canceled" { "canceled" } else { "complete" };
    emit_progress(&app, kind, message, Path::new(""), total.saturating_sub(1), total, 100.0, started);
    Ok(logs)
}

fn main() {
    tauri::Builder::default()
        .manage(Cancellation::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![chdman_status, dependency_status, scan_files, process_batch, cancel_batch])
        .run(tauri::generate_context!()).expect("failed to run application")
}

#[cfg(test)]
mod tests {
    use super::{command_for, detect_extract_command, explicit_extract_format, parse_percent, collides_with_output, reserve_free_directory, is_ascii_path, is_chdman_safe_path, classify_raw_cd_header, is_sector_aligned, sector_size_warning};
    use std::path::{Path, PathBuf};

    #[test]
    fn parses_chdman_progress_messages() {
        assert_eq!(parse_percent("Compressing, 42.7% complete..."), Some(42.7));
        assert_eq!(parse_percent("Verifying, 100% complete"), Some(100.0));
        assert_eq!(parse_percent("No percentage here"), None);
    }

    #[test]
    fn detects_cd_rom_as_cue() {
        let info = "Input file:   test.chd\nFile Version: 5\nMetadata:     Tag='CHT2'  Index=0  Length=89 bytes";
        assert_eq!(detect_extract_command(info), ("extractcd", "cue"));
    }

    #[test]
    fn detects_dvd_as_iso() {
        let info = "Metadata:     Tag='DVD '  Index=0  Length=1 bytes";
        assert_eq!(detect_extract_command(info), ("extractdvd", "iso"));
    }

    #[test]
    fn detects_hard_disk_as_img() {
        let info = "Metadata:     Tag='GDDD'  Index=0  Length=35 bytes";
        assert_eq!(detect_extract_command(info), ("extracthd", "img"));
    }

    #[test]
    fn explicit_formats_override_detection() {
        assert_eq!(explicit_extract_format("cue"), Some(("extractcd", "cue")));
        assert_eq!(explicit_extract_format("bin"), Some(("extractcd", "cue")));
        assert_eq!(explicit_extract_format("iso"), Some(("extractdvd", "iso")));
        assert_eq!(explicit_extract_format("img"), Some(("extracthd", "img")));
        assert_eq!(explicit_extract_format("auto"), None);
        assert_eq!(explicit_extract_format(""), None);
    }

    #[test]
    fn picks_conversion_commands_by_extension() {
        assert_eq!(command_for(Path::new("game.cue"), "convert"), ("createcd", "chd"));
        assert_eq!(command_for(Path::new("movie.iso"), "convert"), ("createdvd", "chd"));
        assert_eq!(command_for(Path::new("hd.img"), "convert"), ("createhd", "chd"));
        assert_eq!(command_for(Path::new("data.raw"), "convert"), ("createraw", "chd"));
        assert_eq!(command_for(Path::new("game.chd"), "verify"), ("verify", ""));
    }

    #[test]
    fn extract_collision_accounts_for_bin_sidecar() {
        let tmp = std::env::temp_dir();
        // Nothing exists at the target -> free.
        let free = tmp.join("free_disc").with_extension("cue");
        assert!(!collides_with_output(&free, "extractcd"));
        // A stray .bin with the same stem must force diversion for extractcd.
        let bin_path = tmp.join("occup_bin").with_extension("bin");
        let _ = std::fs::write(&bin_path, b"x");
        let cue = tmp.join("occup_bin").with_extension("cue");
        assert!(collides_with_output(&cue, "extractcd"));
        // extractdvd has no sidecar: an orphan .bin does not block a .iso.
        let iso = tmp.join("occup_bin").with_extension("iso");
        assert!(!collides_with_output(&iso, "extractdvd"));
        let _ = std::fs::remove_file(&bin_path);
    }

    #[test]
    fn reserve_free_directory_avoids_and_counts_up() {
        let base = std::env::temp_dir().join("chd_reserve_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // First reservation is the plain name.
        let first = reserve_free_directory(&base, "disc");
        assert_eq!(first, base.join("disc"));
        std::fs::create_dir(&first).unwrap();
        // Second goes to "disc (2)".
        let second = reserve_free_directory(&base, "disc");
        assert_eq!(second, base.join("disc (2)"));
        std::fs::create_dir(&second).unwrap();
        // A distinct name is unaffected.
        let other = reserve_free_directory(&base, "other");
        assert_eq!(other, base.join("other"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn path_safety_flags_non_ascii_and_over_long() {
        // Pure ASCII and short is fine.
        let ascii = Path::new(r"C:\roms\metroid.chd");
        assert!(is_ascii_path(ascii));
        assert!(is_chdman_safe_path(ascii));
        // Accented / non-Latin characters disqualify regardless of extension.
        let accented = Path::new(r"C:\Users\Aurélie\Emulátory\jeux été.chd");
        assert!(!is_ascii_path(accented));
        assert!(!is_chdman_safe_path(accented));
        // Overlong, pure-ASCII paths exceed MAX_PATH (260) and must be rejected.
        let long_name = format!("{}{}", "a".repeat(300), ".chd");
        let long = PathBuf::from("C:\\").join(long_name);
        assert!(is_ascii_path(&long), "fully ASCII path should pass the ASCII test");
        assert!(long.as_os_str().len() >= 260, "test fixture must exceed MAX_PATH");
        assert!(!is_chdman_safe_path(&long));
    }

    #[test]
    fn raw_cd_detection_returns_mode_and_rejects_others() {
        // A MODE1/2352 header: 12-byte sync mark then 3-byte MSF + mode byte = 1.
        let mut sector = [0_u8; 16];
        sector[..12].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        sector[12] = 0x00; // MSF frame
        sector[13] = 0x02; // MSF second
        sector[14] = 0x00; // MSF minute
        sector[15] = 0x01; // MODE1
        assert_eq!(classify_raw_cd_header(&sector), Some(1));

        // Same sync mark but MODE2 -> returns 2.
        sector[15] = 0x02;
        assert_eq!(classify_raw_cd_header(&sector), Some(2));

        // Wrong sync mark -> None.
        sector[0] = 0xAB;
        assert_eq!(classify_raw_cd_header(&sector), None);

        // Too short (fewer than the mode-offset) -> None.
        assert_eq!(classify_raw_cd_header(&[0x00, 0xFF]), None);

        // Unknown mode byte (5) -> None.
        let mut weird = [0_u8; 16];
        weird[..12].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        weird[15] = 0x05;
        assert_eq!(classify_raw_cd_header(&weird), None);
    }

    #[test]
    fn sector_alignment_accepts_standard_sizes_and_rejects_stray() {
        assert!(is_sector_aligned(2352));
        assert!(is_sector_aligned(2048));
        assert!(is_sector_aligned(2336));
        assert!(is_sector_aligned(2324));
        assert!(is_sector_aligned(2448));
        assert!(is_sector_aligned(2368));
        assert!(is_sector_aligned(2352 * 32));
        assert!(!is_sector_aligned(0), "zero-length is not aligned");
        assert!(!is_sector_aligned(2353));
        assert!(!is_sector_aligned(2047));
        assert!(!is_sector_aligned(13));
        // A raw CD + subchannel (2352+96=2448) is valid.
        assert!(is_sector_aligned(2448 * 10));

        // sector_size_warning ignores descriptor files regardless of their size.
        let tmp = std::env::temp_dir();
        let cue = tmp.join("sector_check_disc.cue");
        let _ = std::fs::write(&cue, b"x");
        assert_eq!(sector_size_warning(&cue), None, "descriptor files are never warned about");
        // A small broken image triggers a warning.
        let broken = tmp.join("sector_check_broken.iso");
        let _ = std::fs::write(&broken, b"12345");
        assert!(sector_size_warning(&broken).is_some());
        let _ = std::fs::remove_file(&cue);
        let _ = std::fs::remove_file(&broken);
    }
}
