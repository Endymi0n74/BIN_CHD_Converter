#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tempfile::TempDir;
use walkdir::WalkDir;

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
    if mode == "convert" { &["cue", "gdi", "toc", "iso", "img", "raw", "cso", "ciso", "zip", "7z", "rar", "pbp", "ccd"] } else { &["chd"] }
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

fn command_for(path: &Path, mode: &str) -> (&'static str, &'static str) {
    if mode == "verify" { return ("verify", ""); }
    match path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "cue" | "gdi" | "toc" => ("createcd", "chd"),
        "iso" => ("createdvd", "chd"),
        "raw" => ("createraw", "chd"),
        _ => ("createhd", "chd"),
    }
}

fn extraction_command(chdman: &Path, input: &Path) -> (&'static str, &'static str) {
    let info = Command::new(chdman).arg("info").arg("-i").arg(input).output();
    let text = info.map(|value| format!("{}\n{}", String::from_utf8_lossy(&value.stdout), String::from_utf8_lossy(&value.stderr)).to_ascii_lowercase()).unwrap_or_default();
    if text.contains("dvd") { ("extractdvd", "iso") }
    else if text.contains("hard disk") || text.contains("hdd") { ("extracthd", "img") }
    else if text.contains("gd-rom") { ("extractcd", "gdi") }
    else { ("extractcd", "cue") }
}

fn prepared_inputs(input: &Path) -> Result<(Vec<PathBuf>, Option<TempDir>), String> {
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

fn run_streamed(app: &AppHandle, command: &mut Command, input: &Path, index: usize, total: usize, started: Instant) -> Result<bool, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let shown = format!("$ {:?}", command);
    emit_progress(app, "command", &shown, input, index, total, 0.0, started);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let (sender, receiver) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() { stream_reader(stdout, sender.clone()); }
    if let Some(stderr) = child.stderr.take() { stream_reader(stderr, sender.clone()); }
    drop(sender);
    let mut last_percent = 0.0;
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if let Some(percent) = parse_percent(&line) { last_percent = percent; }
                emit_progress(app, if parse_percent(&line).is_some() { "progress" } else { "output" }, &line, input, index, total, last_percent, started);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    while let Ok(line) = receiver.try_recv() { emit_progress(app, "output", &line, input, index, total, last_percent, started); }
                    return Ok(status.success());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return child.wait().map(|status| status.success()).map_err(|e| e.to_string()),
        }
    }
}

#[tauri::command]
fn process_batch(app: AppHandle, source: String, output: String, mode: String, recursive: bool, delete_source: bool) -> Result<Vec<String>, String> {
    let chdman = find_command(&["chdman"]).ok_or("chdman was not found. Install MAME first.")?;
    let files = scan_files(source.clone(), mode.clone(), recursive)?;
    let total = files.len();
    let started = Instant::now();
    let mut logs = Vec::new();
    for (index, item) in files.into_iter().enumerate() {
        let original = PathBuf::from(&item);
        emit_progress(&app, "start", "Preparing input", &original, index, total, 0.0, started);
        let (inputs, _temp) = prepared_inputs(&original)?;
        let mut all_ok = true;
        for input in inputs {
            let (cmd, ext) = if mode == "extract" { extraction_command(&chdman, &input) } else { command_for(&input, &mode) };
            let mut command = Command::new(&chdman);
            command.arg(cmd).arg("-i").arg(&input);
            let out = if mode == "verify" { None } else {
                let rel = if input.starts_with(&source) { input.strip_prefix(&source).unwrap_or(&input) } else { Path::new(input.file_name().unwrap_or_default()) };
                let target = Path::new(&output).join(rel).with_extension(ext);
                if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                let unique = unique_path(target);
                command.arg("-o").arg(&unique);
                Some(unique)
            };
            if run_streamed(&app, &mut command, &input, index, total, started)? {
                let message = format!("OK: {}", input.display());
                logs.push(message.clone());
                emit_progress(&app, "success", &message, &input, index, total, 100.0, started);
            } else {
                all_ok = false;
                let message = format!("FAILED: {}", input.display());
                logs.push(message.clone());
                emit_progress(&app, "error", &message, &input, index, total, 100.0, started);
                if let Some(path) = out { let _ = fs::remove_file(path); }
            }
        }
        if all_ok && delete_source { fs::remove_file(&original).map_err(|e| e.to_string())?; }
    }
    emit_progress(&app, "complete", "Batch complete", Path::new(""), total.saturating_sub(1), total, 100.0, started);
    Ok(logs)
}

fn main() {
    tauri::Builder::default().plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![chdman_status, dependency_status, scan_files, process_batch])
        .run(tauri::generate_context!()).expect("failed to run application")
}

#[cfg(test)]
mod tests {
    use super::parse_percent;

    #[test]
    fn parses_chdman_progress_messages() {
        assert_eq!(parse_percent("Compressing, 42.7% complete..."), Some(42.7));
        assert_eq!(parse_percent("Verifying, 100% complete"), Some(100.0));
        assert_eq!(parse_percent("No percentage here"), None);
    }
}
