use crate::error::LauncherError;
use crate::platform::xdg_data_home;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const DESKTOP_FILENAME: &str = "io.github.checkpickerupper.RobloxStudioLinuxLauncher.auth.desktop";
const GUI_DESKTOP_FILENAME: &str = "io.github.checkpickerupper.RobloxStudioLinuxLauncher.desktop";
const AUTH_HANDLER: &str = "x-scheme-handler/roblox-studio-auth";
const MIME_CACHE_FILENAME: &str = "mimeinfo.cache";
const XDG_MIME_COMMAND: &str = "xdg-mime";
const FLATPAK_SPAWN_PATH: &str = "/usr/bin/flatpak-spawn";
const ICON_FILENAME: &str = "io.github.checkpickerupper.RobloxStudioLinuxLauncher.png";
const ICON_BYTES: &[u8] =
    include_bytes!("../assets/io.github.checkpickerupper.RobloxStudioLinuxLauncher.png");
const GUI_DESKTOP_ENTRY: &str =
    include_str!("../assets/io.github.checkpickerupper.RobloxStudioLinuxLauncher.desktop");
#[cfg(test)]
const AUTH_DESKTOP_ENTRY: &str =
    include_str!("../assets/io.github.checkpickerupper.RobloxStudioLinuxLauncher.auth.desktop");

pub(crate) fn register_auth_handler() -> Result<(), LauncherError> {
    let in_flatpak = env::var_os("FLATPAK_ID").is_some();
    if !in_flatpak {
        let executable = env::current_exe()
            .map_err(|source| LauncherError::ResolveCurrentExecutable { source })?;
        let applications_directory = data_directory().join("applications");
        fs::create_dir_all(&applications_directory).map_err(|source| {
            LauncherError::CreateDesktopDirectory {
                path: applications_directory.clone(),
                source,
            }
        })?;

        install_launcher_icon()?;

        let gui_desktop_path = applications_directory.join(GUI_DESKTOP_FILENAME);
        fs::write(&gui_desktop_path, GUI_DESKTOP_ENTRY).map_err(|source| {
            LauncherError::WriteDesktopEntry {
                path: gui_desktop_path,
                source,
            }
        })?;

        let desktop_path = applications_directory.join(DESKTOP_FILENAME);
        let desktop_contents = desktop_entry(&executable);
        fs::write(&desktop_path, desktop_contents).map_err(|source| {
            LauncherError::WriteDesktopEntry {
                path: desktop_path,
                source,
            }
        })?;
        update_mime_cache(&applications_directory.join(MIME_CACHE_FILENAME))?;
    }

    let program = xdg_mime_program(in_flatpak);
    let status = xdg_mime_command(in_flatpak)
        .args(["default", DESKTOP_FILENAME, AUTH_HANDLER])
        .status()
        .map_err(|source| LauncherError::RunDesktopRegistration {
            program: program.to_owned(),
            source,
        })?;
    if !status.success() {
        return Err(LauncherError::DesktopRegistrationFailed {
            program: program.to_owned(),
            exit_code: status.code().unwrap_or(1),
        });
    }

    let query = xdg_mime_command(in_flatpak)
        .args(["query", "default", AUTH_HANDLER])
        .output()
        .map_err(|source| LauncherError::RunDesktopRegistration {
            program: program.to_owned(),
            source,
        })?;
    if !query.status.success() {
        return Err(LauncherError::DesktopRegistrationFailed {
            program: program.to_owned(),
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
        handler = DESKTOP_FILENAME,
        "Registered Roblox Studio browser login handler"
    );
    Ok(())
}

fn xdg_mime_command(in_flatpak: bool) -> Command {
    if in_flatpak {
        let mut command = Command::new(FLATPAK_SPAWN_PATH);
        command.args(["--host", XDG_MIME_COMMAND]);
        command
    } else {
        Command::new(XDG_MIME_COMMAND)
    }
}

const fn xdg_mime_program(in_flatpak: bool) -> &'static str {
    if in_flatpak {
        "flatpak-spawn --host xdg-mime"
    } else {
        XDG_MIME_COMMAND
    }
}

fn data_directory() -> PathBuf {
    xdg_data_home()
}

fn install_launcher_icon() -> Result<(), LauncherError> {
    let icon_directory = data_directory()
        .join("icons")
        .join("hicolor")
        .join("512x512")
        .join("apps");
    fs::create_dir_all(&icon_directory).map_err(|source| {
        LauncherError::CreateDesktopDirectory {
            path: icon_directory.clone(),
            source,
        }
    })?;
    let icon_path = icon_directory.join(ICON_FILENAME);
    fs::write(&icon_path, ICON_BYTES).map_err(|source| LauncherError::WriteDesktopIcon {
        path: icon_path,
        source,
    })
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
         Icon=io.github.checkpickerupper.RobloxStudioLinuxLauncher\n\
         Terminal=false\n\
         Categories=Development;\n\
         StartupWMClass=roblox-studio-linux-launcher\n\
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

#[cfg(test)]
mod tests {
    use super::{xdg_mime_command, AUTH_DESKTOP_ENTRY};
    use behave::prelude::*;

    behave! {
        "Returning from browser sign-in" {
            "the packaged Flatpak callback entry" {
                "starts the launcher command inside the sandbox" {
                    expect!(AUTH_DESKTOP_ENTRY.contains(
                        "Exec=roblox-studio-linux-launcher launch %u"
                    )).to_be_true()?;
                    expect!(AUTH_DESKTOP_ENTRY.contains("flatpak run")).to_be_false()?;
                }
            }

            "the launcher is running inside Flatpak" {
                "registers and verifies the callback on the Linux host" {
                    let command = xdg_mime_command(true);
                    let program = command.get_program().to_string_lossy().into_owned();
                    let arguments = command
                        .get_args()
                        .map(|argument| argument.to_string_lossy().into_owned())
                        .collect::<Vec<_>>();

                    expect!(program).to_equal("/usr/bin/flatpak-spawn".to_owned())?;
                    expect!(arguments)
                        .to_equal(vec!["--host".to_owned(), "xdg-mime".to_owned()])?;
                }
            }
        }
    }
}
