use std::{env, fs, path::{Path, PathBuf}, process::Command};
use tempfile::TempDir;
use walkdir::WalkDir;
use serde::Serialize;

fn find_command(names: &[&str]) -> Option<PathBuf> {
    let mut roots = vec![env::current_exe().ok()?.parent()?.to_path_buf()];
    roots.extend(env::var_os("PATH").as_deref().map(env::split_paths).into_iter().flatten());
    for root in roots { for name in names { let candidate = root.join(if cfg!(windows) && !name.ends_with(".exe") { format!("{name}.exe") } else { (*name).into() }); if candidate.is_file() { return Some(candidate); } } }
    None
}

#[tauri::command] fn chdman_status() -> bool { find_command(&["chdman"]).is_some() }
#[derive(Serialize)] struct DependencyStatus { chdman: bool, seven_zip: bool, maxcso: bool }
#[tauri::command] fn dependency_status() -> DependencyStatus { DependencyStatus { chdman: chdman_status(), seven_zip: find_command(&["7zz","7z","7za"]).is_some(), maxcso: find_command(&["maxcso"]).is_some() } }
fn extensions(mode: &str) -> &'static [&'static str] { if mode == "convert" { &["cue","gdi","toc","iso","img","raw","cso","ciso","zip","7z","rar","pbp","ccd"] } else { &["chd"] } }

#[tauri::command]
fn scan_files(folder: String, mode: String, recursive: bool) -> Result<Vec<String>, String> {
    let depth = if recursive { usize::MAX } else { 1 };
    let mut files: Vec<_> = WalkDir::new(folder).max_depth(depth).into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file()).filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| extensions(&mode).contains(&x.to_ascii_lowercase().as_str())))
        .map(|e| e.path().display().to_string()).collect();
    files.sort(); Ok(files)
}

fn unique_path(path: PathBuf) -> PathBuf { if !path.exists() { return path; } let dir=path.parent().unwrap_or(Path::new("")); let stem=path.file_stem().unwrap_or_default().to_string_lossy(); let ext=path.extension().map(|x|format!(".{}",x.to_string_lossy())).unwrap_or_default(); for n in 1.. { let p=dir.join(format!("{stem} ({n}){ext}")); if !p.exists(){return p;} } unreachable!() }
fn command_for(path: &Path, mode: &str) -> (&'static str, &'static str) { if mode=="verify" { return ("verify",""); } match path.extension().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase().as_str(){"cue"|"gdi"|"toc"=>("createcd","chd"),"iso"=>("createdvd","chd"),"raw"=>("createraw","chd"),_=>("createhd","chd")} }

fn extraction_command(chdman: &Path, input: &Path) -> (&'static str, &'static str) {
    let info = Command::new(chdman).arg("info").arg("-i").arg(input).output();
    let text = info.map(|value| format!("{}\n{}", String::from_utf8_lossy(&value.stdout), String::from_utf8_lossy(&value.stderr)).to_ascii_lowercase()).unwrap_or_default();
    if text.contains("dvd") { ("extractdvd", "iso") }
    else if text.contains("hard disk") || text.contains("hdd") { ("extracthd", "img") }
    else if text.contains("gd-rom") { ("extractcd", "gdi") }
    else { ("extractcd", "cue") }
}

fn prepared_inputs(input: &Path) -> Result<(Vec<PathBuf>, Option<TempDir>), String> {
    let ext=input.extension().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext=="pbp" || ext=="ccd" { let helper=find_command(&["batch-format-helper"]).ok_or("PBP/CCD support requires batch-format-helper beside the app")?; let temp=TempDir::new().map_err(|e|e.to_string())?; let result=Command::new(helper).arg(&ext).arg(input).arg(temp.path()).output().map_err(|e|e.to_string())?; if !result.status.success(){return Err(String::from_utf8_lossy(&result.stderr).into_owned());} let files=String::from_utf8_lossy(&result.stdout).lines().map(PathBuf::from).collect(); return Ok((files,Some(temp))); }
    if ext=="cso" || ext=="ciso" { let temp=TempDir::new().map_err(|e|e.to_string())?; if let Some(helper)=find_command(&["batch-format-helper"]){let result=Command::new(helper).arg("cso").arg(input).arg(temp.path()).output().map_err(|e|e.to_string())?;if !result.status.success(){return Err(String::from_utf8_lossy(&result.stderr).into_owned());}let files=String::from_utf8_lossy(&result.stdout).lines().map(PathBuf::from).collect();return Ok((files,Some(temp)));}let tool=find_command(&["maxcso"]).ok_or("CSO support requires batch-format-helper or maxcso")?;let iso=temp.path().join(format!("{}.iso",input.file_stem().unwrap_or_default().to_string_lossy()));let result=Command::new(tool).arg("--decompress").arg(input).arg("-o").arg(&iso).output().map_err(|e|e.to_string())?;if !result.status.success(){return Err(String::from_utf8_lossy(&result.stderr).into_owned());}return Ok((vec![iso],Some(temp))); }
    if ["zip","7z","rar"].contains(&ext.as_str()) { let tool=find_command(&["7zz","7z","7za"]).ok_or("Archive support requires 7zz/7z in PATH or beside the app")?; let temp=TempDir::new().map_err(|e|e.to_string())?; let result=Command::new(tool).arg("x").arg(input).arg(format!("-o{}",temp.path().display())).arg("-y").output().map_err(|e|e.to_string())?; if !result.status.success(){return Err(String::from_utf8_lossy(&result.stderr).into_owned());} let files=WalkDir::new(temp.path()).into_iter().filter_map(Result::ok).filter(|e|e.file_type().is_file()).filter(|e|e.path().extension().and_then(|x|x.to_str()).is_some_and(|x|["cue","gdi","toc","iso","img","raw"].contains(&x.to_ascii_lowercase().as_str()))).map(|e|e.into_path()).collect(); return Ok((files,Some(temp))); }
    Ok((vec![input.to_path_buf()],None))
}

#[tauri::command]
fn process_batch(source:String, output:String, mode:String, recursive:bool, delete_source:bool) -> Result<Vec<String>,String> {
    let chdman=find_command(&["chdman"]).ok_or("chdman was not found. Install MAME first.")?; let files=scan_files(source.clone(),mode.clone(),recursive)?; let mut logs=Vec::new();
    for item in files { let original=PathBuf::from(&item); let (inputs,_temp)=prepared_inputs(&original)?; let mut all_ok=true; for input in inputs { let (cmd,ext)=if mode=="extract"{extraction_command(&chdman,&input)}else{command_for(&input,&mode)}; let mut command=Command::new(&chdman); command.arg(cmd).arg("-i").arg(&input); let out=if mode=="verify"{None}else{let rel=if input.starts_with(&source){input.strip_prefix(&source).unwrap_or(&input)}else{Path::new(input.file_name().unwrap_or_default())};let target=Path::new(&output).join(rel).with_extension(ext);if let Some(p)=target.parent(){fs::create_dir_all(p).map_err(|e|e.to_string())?;}let unique=unique_path(target);command.arg("-o").arg(&unique);Some(unique)};let result=command.output().map_err(|e|e.to_string())?;if result.status.success(){logs.push(format!("OK: {}",input.display()));}else{all_ok=false;logs.push(format!("FAILED: {} — {}",input.display(),String::from_utf8_lossy(&result.stderr)));if let Some(p)=out{let _=fs::remove_file(p);}} } if all_ok&&delete_source{fs::remove_file(&original).map_err(|e|e.to_string())?;} }
    Ok(logs)
}

fn main(){tauri::Builder::default().plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![chdman_status,dependency_status,scan_files,process_batch]).run(tauri::generate_context!()).expect("failed to run application")}
