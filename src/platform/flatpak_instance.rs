use crate::error::LauncherError;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

pub(crate) const FLATPAK_APPLICATION_ID: &str =
    "io.github.checkpickerupper.RobloxStudioLinuxLauncher";
const ENTERED_ENVIRONMENT: &str = "ROBLOX_LAUNCHER_FLATPAK_ENTERED";
const STATUS_PATH_ENVIRONMENT: &str = "ROBLOX_LAUNCHER_FLATPAK_STATUS_PATH";
const FLATPAK_SPAWN_PATH: &str = "/usr/bin/flatpak-spawn";
const ENTER_RETRY_COUNT: usize = 20;
const ENTER_RETRY_DELAY: Duration = Duration::from_millis(250);

pub(crate) struct ActiveStudioInvocation {
    config_path: PathBuf,
    launcher_arguments: Vec<OsString>,
    exit_reporting: ExitReporting,
}

enum ExitReporting {
    Process,
    StatusFile(PathBuf),
}

impl ActiveStudioInvocation {
    pub(crate) fn process(
        config_path: &Path,
        launcher_arguments: impl IntoIterator<Item = OsString>,
    ) -> Self {
        Self {
            config_path: config_path.to_path_buf(),
            launcher_arguments: launcher_arguments.into_iter().collect(),
            exit_reporting: ExitReporting::Process,
        }
    }

    pub(crate) fn reported(
        config_path: &Path,
        launcher_arguments: impl IntoIterator<Item = OsString>,
        status_name: &str,
    ) -> Self {
        let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
        let status_path = parent.join(format!(
            ".{status_name}-flatpak-status-{}",
            std::process::id()
        ));
        Self {
            config_path: config_path.to_path_buf(),
            launcher_arguments: launcher_arguments.into_iter().collect(),
            exit_reporting: ExitReporting::StatusFile(status_path),
        }
    }

    pub(crate) fn run_if_needed(self) -> Result<Option<i32>, LauncherError> {
        if env::var_os("FLATPAK_ID").is_none() || env::var_os(ENTERED_ENVIRONMENT).is_some() {
            return Ok(None);
        }

        let instance = wait_for_studio_instance()?;
        let launcher = env::current_exe()
            .map_err(|source| LauncherError::ResolveCurrentExecutable { source })?;
        let status_path = match &self.exit_reporting {
            ExitReporting::Process => None,
            ExitReporting::StatusFile(path) => Some(path.as_path()),
        };
        if let Some(path) = status_path {
            remove_stale_status(path)?;
        }
        let mut command = Command::new(FLATPAK_SPAWN_PATH);
        command.args(["--host", "flatpak", "enter"]);
        command.arg(&instance);
        command.arg("/usr/bin/env");
        append_environment(&mut command, status_path);
        command
            .arg(launcher)
            .arg("--config")
            .arg(absolute_path(&self.config_path))
            .args(self.launcher_arguments);

        tracing::debug!(
            instance,
            "Running command in the active Flatpak Studio sandbox"
        );
        let status =
            command
                .status()
                .map_err(|source| LauncherError::FlatpakInstanceUnavailable {
                    message: format!(
                        "could not enter the running Flatpak Studio sandbox: {source}"
                    ),
                })?;
        match self.exit_reporting {
            ExitReporting::Process => {
                status
                    .code()
                    .map(Some)
                    .ok_or_else(|| LauncherError::FlatpakInstanceUnavailable {
                        message: "the Flatpak command exited without a status code".to_owned(),
                    })
            }
            ExitReporting::StatusFile(path) => read_reported_status(&path).map(Some),
        }
    }
}

pub(crate) fn report_invocation_status(exit_code: i32) {
    let Some(path) = env::var_os(STATUS_PATH_ENVIRONMENT) else {
        return;
    };
    if let Err(error) = fs::write(&path, exit_code.to_string()) {
        tracing::error!(path = %PathBuf::from(path).display(), error = %error, "Could not report the Flatpak command status");
    }
}

pub(crate) fn is_studio_process_command_line(command_line: &str) -> bool {
    let normalized = command_line.to_ascii_lowercase().replace('\\', "/");
    normalized.contains("robloxstudiobeta.exe")
}

fn wait_for_studio_instance() -> Result<String, LauncherError> {
    for attempt in 0..ENTER_RETRY_COUNT {
        if let Some(instance) = find_studio_instance()? {
            return Ok(instance);
        }
        if attempt + 1 < ENTER_RETRY_COUNT {
            thread::sleep(ENTER_RETRY_DELAY);
        }
    }
    Err(LauncherError::FlatpakInstanceUnavailable {
        message: "The launcher GUI must still be open with Roblox Studio running. Open Studio from the launcher, then try again.".to_owned(),
    })
}

fn find_studio_instance() -> Result<Option<String>, LauncherError> {
    let output = run_host_flatpak(["ps", "--columns=instance,application"])?;
    if !output.status.success() {
        return Err(host_failure(
            "could not list running Flatpak instances",
            output,
        ));
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let (Some(instance), Some(application)) = (fields.next(), fields.next()) else {
            continue;
        };
        if application != FLATPAK_APPLICATION_ID {
            continue;
        }
        let process_output = run_host_flatpak(["enter", instance, "/usr/bin/ps", "-eo", "args="])?;
        if process_output.status.success()
            && String::from_utf8_lossy(&process_output.stdout)
                .lines()
                .any(is_studio_process_command_line)
        {
            return Ok(Some(instance.to_owned()));
        }
    }
    Ok(None)
}

fn run_host_flatpak<const N: usize>(arguments: [&str; N]) -> Result<Output, LauncherError> {
    Command::new(FLATPAK_SPAWN_PATH)
        .arg("--host")
        .arg("flatpak")
        .args(arguments)
        .output()
        .map_err(|source| LauncherError::FlatpakInstanceUnavailable {
            message: format!("could not query the Flatpak host: {source}"),
        })
}

fn host_failure(message: &str, output: Output) -> LauncherError {
    let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    LauncherError::FlatpakInstanceUnavailable {
        message: if details.is_empty() {
            message.to_owned()
        } else {
            format!("{message}: {details}")
        },
    }
}

fn append_environment(command: &mut Command, status_path: Option<&Path>) {
    const ENVIRONMENT_KEYS: [&str; 14] = [
        "HOME",
        "PATH",
        "XDG_RUNTIME_DIR",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_SESSION_TYPE",
        "DBUS_SESSION_BUS_ADDRESS",
        "XAUTHORITY",
        "LANG",
        "LANGUAGE",
        "LC_ALL",
    ];
    for key in ENVIRONMENT_KEYS {
        if let Some(value) = env::var_os(key) {
            let mut assignment = OsString::from(key);
            assignment.push("=");
            assignment.push(value);
            command.arg(assignment);
        }
    }
    command.arg(format!("FLATPAK_ID={FLATPAK_APPLICATION_ID}"));
    command.arg(format!("{ENTERED_ENVIRONMENT}=1"));
    if let Some(path) = status_path {
        command.arg(format!("{STATUS_PATH_ENVIRONMENT}={}", path.display()));
    }
}

fn remove_stale_status(path: &Path) -> Result<(), LauncherError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LauncherError::FlatpakInstanceUnavailable {
            message: format!("could not clear the previous Flatpak status file: {error}"),
        }),
    }
}

fn read_reported_status(path: &Path) -> Result<i32, LauncherError> {
    let contents =
        fs::read_to_string(path).map_err(|source| LauncherError::FlatpakInstanceUnavailable {
            message: format!("the Flatpak command did not report its result: {source}"),
        })?;
    if let Err(error) = fs::remove_file(path) {
        tracing::debug!(path = %path.display(), error = %error, "Could not remove the Flatpak status file");
    }
    contents
        .trim()
        .parse::<i32>()
        .map_err(|error| LauncherError::FlatpakInstanceUnavailable {
            message: format!("the Flatpak command reported an invalid exit code: {error}"),
        })
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match env::current_dir() {
        Ok(directory) => directory.join(path),
        Err(error) => {
            tracing::debug!(error = %error, "Could not resolve the current directory");
            path.to_path_buf()
        }
    }
}
