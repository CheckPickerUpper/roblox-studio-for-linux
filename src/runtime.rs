use crate::error::LauncherError;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

pub fn resolve_wine_binary(configured_binary: &str) -> Option<PathBuf> {
    if configured_binary.contains('/') {
        let executable = PathBuf::from(configured_binary);
        return executable.is_file().then_some(executable);
    }

    let path_entries = env::var_os("PATH")?;
    env::split_paths(&path_entries)
        .map(|directory| directory.join(configured_binary))
        .find(|candidate| candidate.is_file())
}

pub fn discover_studio_executable(wine_prefix: &Path) -> Result<Option<PathBuf>, LauncherError> {
    let windows_drive = wine_prefix.join("drive_c");
    if !windows_drive.is_dir() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    collect_studio_executables(&windows_drive, &mut candidates)?;
    candidates.sort_by_key(|candidate| {
        fs::metadata(candidate)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    Ok(candidates.pop())
}

pub fn run_wine(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    fs::create_dir_all(wine_prefix).map_err(|source| LauncherError::CreateWinePrefix {
        path: wine_prefix.to_path_buf(),
        source,
    })?;

    let mut command = Command::new(wine_binary);
    command.env("WINEPREFIX", wine_prefix);
    command.args(arguments);
    let status = command.status().map_err(|source| LauncherError::RunWine {
        program: wine_binary.display().to_string(),
        source,
    })?;
    Ok(status.code().unwrap_or(1))
}

fn collect_studio_executables(
    directory: &Path,
    candidates: &mut Vec<PathBuf>,
) -> Result<(), LauncherError> {
    let entries = fs::read_dir(directory).map_err(|source| LauncherError::ReadDirectory {
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LauncherError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_studio_executables(&path, candidates)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("RobloxStudioBeta.exe") {
            candidates.push(path);
        }
    }
    Ok(())
}
