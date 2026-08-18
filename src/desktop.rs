use crate::error::LauncherError;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const DESKTOP_FILENAME: &str = "roblox-studio-linux-launcher.desktop";
const AUTH_HANDLER: &str = "x-scheme-handler/roblox-studio-auth";
const MIME_CACHE_FILENAME: &str = "mimeinfo.cache";
const XDG_MIME_COMMAND: &str = "xdg-mime";

pub(crate) fn register_auth_handler() -> Result<(), LauncherError> {
    let executable =
        env::current_exe().map_err(|source| LauncherError::ResolveCurrentExecutable { source })?;
    let applications_directory = data_directory().join("applications");
    fs::create_dir_all(&applications_directory).map_err(|source| {
        LauncherError::CreateDesktopDirectory {
            path: applications_directory.clone(),
            source,
        }
    })?;

    let desktop_path = applications_directory.join(DESKTOP_FILENAME);
    let desktop_contents = desktop_entry(&executable);
    fs::write(&desktop_path, desktop_contents).map_err(|source| {
        LauncherError::WriteDesktopEntry {
            path: desktop_path.clone(),
            source,
        }
    })?;
    update_mime_cache(&applications_directory.join(MIME_CACHE_FILENAME))?;

    let status = Command::new(XDG_MIME_COMMAND)
        .args(["default", DESKTOP_FILENAME, AUTH_HANDLER])
        .status()
        .map_err(|source| LauncherError::RunDesktopRegistration {
            program: XDG_MIME_COMMAND.to_owned(),
            source,
        })?;
    if !status.success() {
        return Err(LauncherError::DesktopRegistrationFailed {
            program: XDG_MIME_COMMAND.to_owned(),
            exit_code: status.code().unwrap_or(1),
        });
    }

    let query = Command::new(XDG_MIME_COMMAND)
        .args(["query", "default", AUTH_HANDLER])
        .output()
        .map_err(|source| LauncherError::RunDesktopRegistration {
            program: XDG_MIME_COMMAND.to_owned(),
            source,
        })?;
    if !query.status.success() {
        return Err(LauncherError::DesktopRegistrationFailed {
            program: XDG_MIME_COMMAND.to_owned(),
            exit_code: query.status.code().unwrap_or(1),
        });
    }

    let actual = String::from_utf8_lossy(&query.stdout).trim().to_owned();
    if actual != DESKTOP_FILENAME {
        return Err(LauncherError::DesktopRegistrationMismatch {
            expected: DESKTOP_FILENAME.to_owned(),
            actual,
        });
    }

    tracing::info!(
        path = %desktop_path.display(),
        "Registered Roblox Studio browser login handler"
    );
    Ok(())
}

fn data_directory() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn update_mime_cache(path: &Path) -> Result<(), LauncherError> {
    let existing = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(LauncherError::ReadDesktopMimeCache {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let mut lines = existing.lines().map(str::to_owned).collect::<Vec<_>>();
    let mime_cache_header = "[MIME Cache]";
    let mapping_prefix = format!("{AUTH_HANDLER}=");
    let header_index = lines.iter().position(|line| line == mime_cache_header);
    let mapping_index = header_index.and_then(|header_index| {
        lines
            .iter()
            .enumerate()
            .skip(header_index + 1)
            .take_while(|(_, line)| !line.starts_with('['))
            .find_map(|(index, line)| line.starts_with(&mapping_prefix).then_some(index))
    });

    let mapping = match mapping_index {
        Some(index) => {
            let mut entries = lines[index][mapping_prefix.len()..]
                .split(';')
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if !entries.iter().any(|entry| entry == DESKTOP_FILENAME) {
                entries.push(DESKTOP_FILENAME.to_owned());
            }
            lines[index] = format!("{mapping_prefix}{};", entries.join(";"));
            lines[index].clone()
        }
        None => {
            let line = format!("{mapping_prefix}{DESKTOP_FILENAME};");
            match header_index {
                Some(index) => lines.insert(index + 1, line.clone()),
                None => {
                    lines.insert(0, mime_cache_header.to_owned());
                    lines.insert(1, line.clone());
                }
            }
            line
        }
    };

    let mut updated = lines.join("\n");
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    fs::write(path, updated).map_err(|source| LauncherError::WriteDesktopMimeCache {
        path: path.to_path_buf(),
        source,
    })?;

    tracing::debug!(path = %path.display(), mapping = %mapping, "Updated desktop MIME cache");
    Ok(())
}

fn desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Roblox Studio (Unofficial Linux Launcher)\n\
         Comment=Launch Roblox Studio through Wine\n\
         Exec={} launch %u\n\
         Icon=application-x-executable\n\
         Terminal=false\n\
         Categories=Development;\n\
         MimeType={AUTH_HANDLER};\n",
        desktop_exec_value(executable),
    )
}

fn desktop_exec_value(executable: &Path) -> String {
    let executable = executable.to_string_lossy();
    if !executable.chars().any(char::is_whitespace) {
        return executable.replace('%', "%%");
    }

    let mut value = String::from("\"");
    for character in executable.chars() {
        match character {
            '\\' | '"' => {
                value.push('\\');
                value.push(character);
            }
            '%' => value.push_str("%%"),
            _ => value.push(character),
        }
    }
    value.push('"');
    value
}
