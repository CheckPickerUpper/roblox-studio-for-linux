use crate::error::LauncherError;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub(crate) fn resolve_wine_binary(configured_binary: &str) -> Option<PathBuf> {
    if configured_binary.contains('/') {
        let executable = PathBuf::from(configured_binary);
        return executable.is_file().then_some(executable);
    }

    let path_entries = env::var_os("PATH")?;
    env::split_paths(&path_entries)
        .map(|directory| directory.join(configured_binary))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn discover_studio_executable(
    wine_prefix: &Path,
) -> Result<Option<PathBuf>, LauncherError> {
    let windows_drive = wine_prefix.join("drive_c");
    if !windows_drive.is_dir() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    collect_studio_executables(&windows_drive, &mut candidates)?;
    candidates.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(candidates.pop().map(|candidate| candidate.path))
}

pub(crate) fn run_wine(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    let (command, program) = create_wine_command(wine_binary, wine_prefix, arguments)?;
    run_wine_command(command, program)
}

pub(crate) fn configure_wine_prefix(
    wine_binary: &Path,
    wine_prefix: &Path,
) -> Result<i32, LauncherError> {
    run_wine(
        wine_binary,
        wine_prefix,
        &[
            "winecfg.exe".to_owned(),
            "-v".to_owned(),
            "win10".to_owned(),
        ],
    )
}

pub(crate) fn configure_webview2_runtime(
    wine_binary: &Path,
    wine_prefix: &Path,
) -> Result<i32, LauncherError> {
    run_wine(
        wine_binary,
        wine_prefix,
        &[
            "reg.exe".to_owned(),
            "ADD".to_owned(),
            r"HKCU\Software\Wine\AppDefaults\msedgewebview2.exe".to_owned(),
            "/v".to_owned(),
            "Version".to_owned(),
            "/d".to_owned(),
            "win7".to_owned(),
            "/f".to_owned(),
        ],
    )
}

pub(crate) fn ensure_webview2_runtime(
    wine_binary: &Path,
    wine_prefix: &Path,
    studio_executable: &Path,
) -> Result<i32, LauncherError> {
    let runtime_directory = wine_prefix
        .join("drive_c")
        .join("Program Files (x86)")
        .join("Microsoft")
        .join("EdgeWebView")
        .join("Application");
    if webview2_runtime_is_installed(&runtime_directory)? {
        tracing::debug!(
            path = %runtime_directory.display(),
            "WebView2 runtime is already installed"
        );
        return Ok(0);
    }

    let studio_directory =
        studio_executable
            .parent()
            .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
                path: studio_executable.to_path_buf(),
            })?;
    let installer = studio_directory
        .join("WebView2RuntimeInstaller")
        .join("MicrosoftEdgeWebview2Setup.exe");
    if !installer.is_file() {
        return Err(LauncherError::MissingWebView2Installer { path: installer });
    }

    tracing::info!(path = %installer.display(), "Installing WebView2 runtime");
    let exit_code = run_wine(
        wine_binary,
        wine_prefix,
        &[
            installer.display().to_string(),
            "/silent".to_owned(),
            "/install".to_owned(),
        ],
    )?;
    if exit_code != 0 {
        tracing::error!(exit_code, "WebView2 installer exited unsuccessfully");
        return Ok(exit_code);
    }
    if !webview2_runtime_is_installed(&runtime_directory)? {
        return Err(LauncherError::MissingWebView2Runtime {
            path: runtime_directory,
        });
    }
    Ok(0)
}

pub(crate) fn run_studio_auth(
    wine_binary: &Path,
    wine_prefix: &Path,
    studio_executable: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    let (mut command, program) = create_wine_command(wine_binary, wine_prefix, &[])?;
    command.arg(studio_executable);
    command.args(arguments);
    configure_studio_environment(&mut command);
    run_wine_command(command, program)
}

fn create_wine_command(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
) -> Result<(Command, String), LauncherError> {
    fs::create_dir_all(wine_prefix).map_err(|source| LauncherError::CreateWinePrefix {
        path: wine_prefix.to_path_buf(),
        source,
    })?;

    let program = wine_binary.display().to_string();
    let mut command = Command::new(wine_binary);
    command.env("WINEPREFIX", wine_prefix);
    command.args(arguments);
    Ok((command, program))
}

fn run_wine_command(mut command: Command, program: String) -> Result<i32, LauncherError> {
    let status = command.status().map_err(|source| LauncherError::RunWine {
        program: program.clone(),
        source,
    })?;
    match status.code() {
        Some(exit_code) => Ok(exit_code),
        None => Err(LauncherError::WineProcessExitedWithoutCode { program }),
    }
}

pub(crate) fn run_studio(
    wine_binary: &Path,
    wine_prefix: &Path,
    studio_executable: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    let (mut command, program) = create_wine_command(wine_binary, wine_prefix, &[])?;

    let studio_directory =
        studio_executable
            .parent()
            .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
                path: studio_executable.to_path_buf(),
            })?;
    let executable_name = studio_executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
            path: studio_executable.to_path_buf(),
        })?;
    let windows_studio_directory = to_windows_drive_path(wine_prefix, studio_directory)?;
    let windows_qt_directory = format!("{}\\Qt5", windows_studio_directory);
    let windows_qt_plugins = format!("{}\\platforms", windows_qt_directory);

    validate_cmd_value(&windows_studio_directory)?;
    validate_cmd_value(&windows_qt_directory)?;
    validate_cmd_value(&windows_qt_plugins)?;
    validate_cmd_value(executable_name)?;

    let mut command_line = format!(
        "cd /d {}&set QT_QPA_PLATFORM_PLUGIN_PATH={}&set QT_PLUGIN_PATH={}&{}",
        windows_studio_directory, windows_qt_plugins, windows_qt_directory, executable_name,
    );
    for argument in arguments {
        validate_cmd_value(argument)?;
        command_line.push(' ');
        command_line.push_str(&quote_cmd_value(argument));
    }

    command.args(["cmd.exe", "/d", "/s", "/c"]);
    command.arg(command_line);
    configure_studio_environment(&mut command);
    run_wine_command(command, program)
}

fn configure_studio_environment(command: &mut Command) {
    command.env("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--disable-gpu");
    let mut wine_dll_overrides = env::var("WINEDLLOVERRIDES").unwrap_or_default();
    if !wine_dll_overrides.is_empty() {
        wine_dll_overrides.push(';');
    }
    wine_dll_overrides.push_str("dxdiagn,winemenubuilder.exe,mscoree,mshtml=");
    command.env("WINEDLLOVERRIDES", wine_dll_overrides);
}

fn webview2_runtime_is_installed(runtime_directory: &Path) -> Result<bool, LauncherError> {
    if !runtime_directory.is_dir() {
        return Ok(false);
    }
    let entries =
        fs::read_dir(runtime_directory).map_err(|source| LauncherError::ReadDirectory {
            path: runtime_directory.to_path_buf(),
            source,
        })?;
    for entry in entries {
        let entry = entry.map_err(|source| LauncherError::ReadDirectory {
            path: runtime_directory.to_path_buf(),
            source,
        })?;
        if entry.path().join("msedgewebview2.exe").is_file() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn to_windows_drive_path(wine_prefix: &Path, path: &Path) -> Result<String, LauncherError> {
    let wine_drive = wine_prefix.join("drive_c");
    let relative_path = path.strip_prefix(&wine_drive).map_err(|_| {
        LauncherError::StudioExecutableOutsideWineDrive {
            path: path.to_path_buf(),
            wine_drive: wine_drive.clone(),
        }
    })?;

    let mut windows_path = String::from("C:");
    for component in relative_path.components() {
        let Component::Normal(segment) = component else {
            return Err(LauncherError::InvalidStudioLaunchPath {
                path: path.to_path_buf(),
            });
        };
        let segment = segment
            .to_str()
            .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
                path: path.to_path_buf(),
            })?;
        validate_cmd_value(segment)?;
        windows_path.push('\\');
        windows_path.push_str(segment);
    }
    Ok(windows_path)
}

fn validate_cmd_value(value: &str) -> Result<(), LauncherError> {
    if value.chars().any(|character| {
        matches!(
            character,
            '&' | '|' | '<' | '>' | '^' | '%' | '!' | '"' | '\r' | '\n'
        )
    }) {
        return Err(LauncherError::InvalidStudioLaunchValue {
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn quote_cmd_value(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    quoted.push_str(value);
    quoted.push('"');
    quoted
}

struct StudioCandidate {
    path: PathBuf,
    modified: SystemTime,
}

fn collect_studio_executables(
    directory: &Path,
    candidates: &mut Vec<StudioCandidate>,
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
            let metadata =
                fs::metadata(&path).map_err(|source| LauncherError::ReadFileMetadata {
                    path: path.clone(),
                    source,
                })?;
            let modified =
                metadata
                    .modified()
                    .map_err(|source| LauncherError::ReadFileMetadata {
                        path: path.clone(),
                        source,
                    })?;
            candidates.push(StudioCandidate { path, modified });
        }
    }
    Ok(())
}
