use crate::config::StudioLoginMode;
use crate::error::LauncherError;
use ring::digest::{Context as DigestContext, SHA256};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use url::Url;

const QUOTED_VALUE_DELIMITER_COUNT: usize = 2;
const WEBVIEW2_STABLE_CLIENT_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
const STUDIO_CLIENT_SETTINGS_FILENAME: &str = "ClientAppSettings.json";
const STUDIO_BROWSER_HISTORY_FILENAME: &str = "History";
const STUDIO_AUTHORIZATION_URL_PREFIX: &str = "https://apis.roblox.com/oauth/v1/authorize?";
const CHROMIUM_EPOCH_OFFSET_SECONDS: u64 = 11_644_473_600;
const BROWSER_LOGIN_WATCH_TIMEOUT: Duration = Duration::from_secs(90);
const BROWSER_LOGIN_POLL_INTERVAL: Duration = Duration::from_millis(250);
const WINE_SERVER_CHECK_TIMEOUT: Duration = Duration::from_millis(100);
const WINE_SERVER_CHECK_POLL_INTERVAL: Duration = Duration::from_millis(10);
const WINE_SERVER_NOT_RUNNING_EXIT_CODE: i32 = 1;
const MANAGED_WINE_BINARY: &str = "/app/kombucha/bin/wine";
const WINE_DRIVERS_REGISTRY_KEY: &str = r"HKCU\Software\Wine\Drivers";
const WINE_GRAPHICS_VALUE_NAME: &str = "Graphics";
const MANAGED_DXVK_DIRECTORY: &str = "/app/share/roblox-studio-linux-launcher/dxvk/x64";
const MANAGED_DXVK_DLLS: [&str; 5] = [
    "d3d8.dll",
    "d3d9.dll",
    "d3d10core.dll",
    "d3d11.dll",
    "dxgi.dll",
];
const PINNED_WEBVIEW2_VERSION: &str = "144.0.3719.92";
const PINNED_WEBVIEW2_INSTALLER: &str = "MicrosoftEdge_X64_144.0.3719.92.exe";
const PINNED_WEBVIEW2_SIZE: u64 = 185_153_080;
const PINNED_WEBVIEW2_SHA256_BASE64: &str = "dNC7zSOWrDfonkfozjelmPuzIGq9i8b7H33iSz/pJM0=";
const PINNED_WEBVIEW2_SHA256: [u8; 32] = [
    0x74, 0xd0, 0xbb, 0xcd, 0x23, 0x96, 0xac, 0x37, 0xe8, 0x9e, 0x47, 0xe8, 0xce, 0x37, 0xa5, 0x98,
    0xfb, 0xb3, 0x20, 0x6a, 0xbd, 0x8b, 0xc6, 0xfb, 0x1f, 0x7d, 0xe2, 0x4b, 0x3f, 0xe9, 0x24, 0xcd,
];
const WEBVIEW2_DOWNLOAD_API: &str =
    "https://msedge.api.cdp.microsoft.com/api/v1.1/contents/Browser/namespaces/Default/names/msedge-stable-win-x64";
const MICROSOFT_ROOT_CERTIFICATE: &str = include_str!("../assets/microsoft-root-2011.pem");
const WEBVIEW2_DIGEST_BUFFER_SIZE: usize = 1024 * 1024;

/// Owns the complete login/runtime choice for one Studio launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StudioRuntimePlan {
    login_mode: StudioLoginMode,
}

impl StudioRuntimePlan {
    pub(crate) const fn new(login_mode: StudioLoginMode) -> Self {
        Self { login_mode }
    }

    pub(crate) const fn login_mode(self) -> StudioLoginMode {
        self.login_mode
    }

    const fn windows_version(self) -> &'static str {
        "win8"
    }

    const fn browser_arguments(self) -> &'static str {
        "--use-angle=swiftshader"
    }

    const fn wine_graphics_backend(self) -> &'static str {
        "renderer=vulkan"
    }

    const fn wine_dll_overrides(self) -> &'static str {
        "d3d9,d3d10core,d3d11,dxgi=n,b;dxdiagn,winemenubuilder.exe,mscoree,mshtml="
    }

    const fn webview2_version(self) -> &'static str {
        PINNED_WEBVIEW2_VERSION
    }
}

pub(crate) fn resolve_wine_binary(configured_binary: &str) -> Option<PathBuf> {
    resolve_wine_binary_with(
        configured_binary,
        Path::new(MANAGED_WINE_BINARY),
        env::var_os("PATH").as_deref(),
    )
}

fn resolve_wine_binary_with(
    configured_binary: &str,
    managed_binary: &Path,
    path_entries: Option<&OsStr>,
) -> Option<PathBuf> {
    if configured_binary.contains('/') {
        let executable = PathBuf::from(configured_binary);
        return executable.is_file().then_some(executable);
    }

    if configured_binary == "wine" && managed_binary.is_file() {
        return Some(managed_binary.to_path_buf());
    }

    env::split_paths(path_entries?)
        .map(|directory| directory.join(configured_binary))
        .find(|candidate| candidate.is_file())
}

pub(crate) struct StudioInstallation {
    pub(crate) wine_binary: PathBuf,
    pub(crate) wine_prefix: PathBuf,
    pub(crate) studio_version: String,
    pub(crate) studio_executable: PathBuf,
    pub(crate) mcp_executable: PathBuf,
    pub(crate) version_directory: PathBuf,
}

pub(crate) fn discover_studio_installation(
    wine_binary: &Path,
    wine_prefix: &Path,
) -> Result<Option<StudioInstallation>, LauncherError> {
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
    let Some(candidate) = candidates.pop() else {
        return Ok(None);
    };
    let version_directory = match candidate.path.parent() {
        Some(path) => path.to_path_buf(),
        None => {
            return Err(LauncherError::InvalidStudioLaunchPath {
                path: candidate.path,
            });
        }
    };
    let mcp_path = version_directory.join("StudioMCP.exe");
    let studio_version = version_directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
            path: version_directory.clone(),
        })?;
    Ok(Some(StudioInstallation {
        wine_binary: wine_binary.to_path_buf(),
        wine_prefix: wine_prefix.to_path_buf(),
        studio_version,
        studio_executable: candidate.path,
        mcp_executable: mcp_path,
        version_directory,
    }))
}

pub(crate) fn select_studio_installation(
    wine_binary: &Path,
    wine_prefix: &Path,
    fallback: Option<&Path>,
) -> Result<Option<StudioInstallation>, LauncherError> {
    match discover_studio_installation(wine_binary, wine_prefix)? {
        Some(installation) => Ok(Some(installation)),
        None => match fallback {
            Some(path) if path.is_file() => {
                let version_directory = match path.parent() {
                    Some(directory) => directory.to_path_buf(),
                    None => {
                        return Err(LauncherError::InvalidStudioLaunchPath {
                            path: path.to_path_buf(),
                        });
                    }
                };
                let mcp_path = version_directory.join("StudioMCP.exe");
                let studio_version = version_directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
                    .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
                        path: version_directory.clone(),
                    })?;
                Ok(Some(StudioInstallation {
                    wine_binary: wine_binary.to_path_buf(),
                    wine_prefix: wine_prefix.to_path_buf(),
                    studio_version,
                    studio_executable: path.to_path_buf(),
                    mcp_executable: mcp_path,
                    version_directory,
                }))
            }
            Some(_) | None => Ok(None),
        },
    }
}

pub(crate) fn run_wine(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    let (command, program) = create_wine_command(wine_binary, wine_prefix, arguments)?;
    run_wine_command(command, program)
}

pub(crate) fn exec_wine_stdio(
    wine_binary: &Path,
    wine_prefix: &Path,
    executable: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    let (mut command, program) = create_wine_command(wine_binary, wine_prefix, &[])?;
    command.arg(executable);
    command.args(arguments);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let source = command.exec();
        Err(LauncherError::RunWine { program, source })
    }
    #[cfg(not(unix))]
    {
        run_wine_command(command, program)
    }
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

fn wine_graphics_driver_for_display(x11_available: bool) -> &'static str {
    if x11_available {
        "x11,wayland"
    } else {
        "wayland"
    }
}

fn wine_graphics_registry_arguments(graphics_driver: &str) -> Vec<String> {
    vec![
        "reg.exe".to_owned(),
        "ADD".to_owned(),
        WINE_DRIVERS_REGISTRY_KEY.to_owned(),
        "/v".to_owned(),
        WINE_GRAPHICS_VALUE_NAME.to_owned(),
        "/d".to_owned(),
        graphics_driver.to_owned(),
        "/f".to_owned(),
    ]
}

fn saved_wine_graphics_driver(user_registry: &str) -> Option<&str> {
    const DRIVERS_SECTION: &str = r"[Software\\Wine\\Drivers]";
    const GRAPHICS_VALUE_PREFIX: &str = "\"Graphics\"=\"";

    let mut inside_drivers_section = false;
    for line in user_registry.lines() {
        if line.starts_with('[') {
            inside_drivers_section = line
                .strip_prefix(DRIVERS_SECTION)
                .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(' '));
            continue;
        }
        if !inside_drivers_section {
            continue;
        }
        if let Some(value) = line
            .strip_prefix(GRAPHICS_VALUE_PREFIX)
            .and_then(|value| value.strip_suffix('"'))
        {
            return Some(value);
        }
    }
    None
}

fn wine_graphics_driver_needs_update(user_registry: Option<&str>, graphics_driver: &str) -> bool {
    user_registry.and_then(saved_wine_graphics_driver) != Some(graphics_driver)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WineGraphicsPreparation {
    AlreadyConfigured,
    ConfigureAndRestart,
    ActiveSessionConflict,
}

const fn wine_graphics_preparation(
    needs_update: bool,
    wine_server_is_running: bool,
) -> WineGraphicsPreparation {
    match (needs_update, wine_server_is_running) {
        (false, _) => WineGraphicsPreparation::AlreadyConfigured,
        (true, false) => WineGraphicsPreparation::ConfigureAndRestart,
        (true, true) => WineGraphicsPreparation::ActiveSessionConflict,
    }
}

fn configure_wine_graphics_driver(
    wine_binary: &Path,
    wine_prefix: &Path,
    graphics_driver: &str,
) -> Result<i32, LauncherError> {
    let user_registry_path = wine_prefix.join("user.reg");
    let user_registry = match fs::read_to_string(&user_registry_path) {
        Ok(contents) => Some(contents),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(LauncherError::ReadWineRegistry {
                path: user_registry_path,
                source,
            });
        }
    };
    let saved_driver = user_registry
        .as_deref()
        .and_then(saved_wine_graphics_driver)
        .map(str::to_owned);
    let needs_update = wine_graphics_driver_needs_update(user_registry.as_deref(), graphics_driver);
    if !needs_update {
        return Ok(0);
    }
    let wine_server_is_running = wine_server_is_running(wine_binary, wine_prefix)?;
    match wine_graphics_preparation(needs_update, wine_server_is_running) {
        WineGraphicsPreparation::AlreadyConfigured => return Ok(0),
        WineGraphicsPreparation::ConfigureAndRestart => {}
        WineGraphicsPreparation::ActiveSessionConflict => {
            return Err(LauncherError::WineGraphicsChangeWhileRunning {
                prefix: wine_prefix.to_path_buf(),
                saved_driver: saved_driver.unwrap_or_else(|| "unset".to_owned()),
                desired_driver: graphics_driver.to_owned(),
            });
        }
    }

    tracing::info!(
        graphics_driver,
        "Configuring the managed Wine window driver"
    );
    let (command, program) = create_unprepared_wine_command(
        wine_binary,
        wine_prefix,
        &wine_graphics_registry_arguments(graphics_driver),
    )?;
    let exit_code = run_wine_command(command, program)?;
    if exit_code != 0 {
        return Ok(exit_code);
    }
    restart_wine_server(wine_binary, wine_prefix)
}

fn restart_wine_server(wine_binary: &Path, wine_prefix: &Path) -> Result<i32, LauncherError> {
    let wine_server = wine_server_path(wine_binary);
    let program = wine_server.display().to_string();

    for argument in ["-k", "-w"] {
        let mut command = Command::new(&wine_server);
        command.env("WINEPREFIX", wine_prefix).arg(argument);
        let exit_code = run_wine_command(command, program.clone())?;
        let server_was_already_stopped =
            argument == "-k" && exit_code == WINE_SERVER_NOT_RUNNING_EXIT_CODE;
        if exit_code != 0 && !server_was_already_stopped {
            return Ok(exit_code);
        }
    }
    Ok(0)
}

fn wine_server_path(wine_binary: &Path) -> PathBuf {
    let sibling = wine_binary.with_file_name("wineserver");
    if sibling.is_file() {
        sibling
    } else {
        PathBuf::from("wineserver")
    }
}

fn wine_server_is_running(wine_binary: &Path, wine_prefix: &Path) -> Result<bool, LauncherError> {
    let wine_server = wine_server_path(wine_binary);
    let program = wine_server.display().to_string();
    let mut child = Command::new(&wine_server)
        .env("WINEPREFIX", wine_prefix)
        .arg("-w")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| LauncherError::RunWine {
            program: program.clone(),
            source,
        })?;
    let deadline = Instant::now() + WINE_SERVER_CHECK_TIMEOUT;
    loop {
        match child.try_wait().map_err(|source| LauncherError::RunWine {
            program: program.clone(),
            source,
        })? {
            Some(status) if status.success() => return Ok(false),
            Some(status) => {
                return Err(LauncherError::WineServerCheckFailed {
                    program,
                    exit_code: status.code().unwrap_or(WINE_SERVER_NOT_RUNNING_EXIT_CODE),
                });
            }
            None if Instant::now() < deadline => {
                std::thread::sleep(WINE_SERVER_CHECK_POLL_INTERVAL);
            }
            None => {
                child.kill().map_err(|source| LauncherError::RunWine {
                    program: program.clone(),
                    source,
                })?;
                child
                    .wait()
                    .map_err(|source| LauncherError::RunWine { program, source })?;
                return Ok(true);
            }
        }
    }
}

fn configure_webview2_runtime(
    plan: StudioRuntimePlan,
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
            plan.windows_version().to_owned(),
            "/f".to_owned(),
        ],
    )
}

fn set_studio_webview2_override(
    studio_executable: &Path,
    enabled: bool,
) -> Result<bool, LauncherError> {
    let studio_directory =
        studio_executable
            .parent()
            .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
                path: studio_executable.to_path_buf(),
            })?;
    let client_settings_directory = studio_directory.join("ClientSettings");
    fs::create_dir_all(&client_settings_directory).map_err(|source| {
        LauncherError::CreateStudioClientSettingsDirectory {
            path: client_settings_directory.clone(),
            source,
        }
    })?;
    let settings_path = client_settings_directory.join(STUDIO_CLIENT_SETTINGS_FILENAME);
    let mut settings = match fs::read_to_string(&settings_path) {
        Ok(contents) => serde_json::from_str::<Value>(&contents).map_err(|source| {
            LauncherError::ParseStudioClientSettings {
                path: settings_path.clone(),
                source,
            }
        })?,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(source) => {
            return Err(LauncherError::ReadStudioClientSettings {
                path: settings_path,
                source,
            });
        }
    };

    let changed =
        set_studio_client_boolean(&mut settings, "FFlagWebView2", enabled, &settings_path)?;
    if !changed {
        return Ok(false);
    }

    let contents = serde_json::to_vec(&settings).map_err(|source| {
        LauncherError::InvalidStudioClientSettings {
            path: settings_path.clone(),
            message: source.to_string(),
        }
    })?;
    fs::write(&settings_path, contents).map_err(|source| {
        LauncherError::WriteStudioClientSettings {
            path: settings_path.clone(),
            source,
        }
    })?;
    tracing::info!(
        enabled,
        path = %settings_path.display(),
        "Configured Studio WebView2 override"
    );
    Ok(true)
}

pub(crate) fn latest_studio_auth_visit_time(
    wine_prefix: &Path,
) -> Result<Option<i64>, LauncherError> {
    let Some(history_path) = find_studio_webview2_history(wine_prefix)? else {
        return Ok(None);
    };
    read_studio_auth_history(&history_path, i64::MIN).map(|row| row.map(|(_, timestamp)| timestamp))
}

pub(crate) fn watch_for_studio_browser_login(
    wine_prefix: &Path,
    minimum_visit_time: i64,
) -> Result<i32, LauncherError> {
    let deadline = Instant::now() + BROWSER_LOGIN_WATCH_TIMEOUT;
    loop {
        if let Some(history_path) = find_studio_webview2_history(wine_prefix)? {
            match read_studio_auth_history(&history_path, minimum_visit_time) {
                Ok(Some((url, _))) => {
                    open_browser_url(&url)?;
                    tracing::info!(
                        "Opened the current Roblox Studio sign-in page in the Linux browser"
                    );
                    return Ok(0);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(error = %error, "Studio browser history is not ready yet");
                }
            }
        }

        if Instant::now() >= deadline {
            tracing::error!(
                "Studio did not create a browser sign-in URL; the login page may still be loading"
            );
            return Ok(1);
        }
        std::thread::sleep(BROWSER_LOGIN_POLL_INTERVAL);
    }
}

pub(crate) fn chromium_timestamp_now() -> i64 {
    let unix_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let chromium_micros =
        unix_time.as_micros() + u128::from(CHROMIUM_EPOCH_OFFSET_SECONDS) * 1_000_000;
    i64::try_from(chromium_micros).unwrap_or(i64::MAX)
}

fn find_studio_webview2_history(wine_prefix: &Path) -> Result<Option<PathBuf>, LauncherError> {
    let users_directory = wine_prefix.join("drive_c").join("users");
    if !users_directory.is_dir() {
        return Ok(None);
    }
    let entries =
        fs::read_dir(&users_directory).map_err(|source| LauncherError::ReadDirectory {
            path: users_directory.clone(),
            source,
        })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| LauncherError::ReadDirectory {
            path: users_directory.clone(),
            source,
        })?;
        let webview_directory = entry
            .path()
            .join("AppData")
            .join("Local")
            .join("Roblox")
            .join("RobloxStudio")
            .join("WebView2")
            .join("EBWebView");
        if !webview_directory.is_dir() {
            continue;
        }
        let direct_history = webview_directory.join(STUDIO_BROWSER_HISTORY_FILENAME);
        if direct_history.is_file() {
            candidates.push(direct_history);
        }
        let profiles =
            fs::read_dir(&webview_directory).map_err(|source| LauncherError::ReadDirectory {
                path: webview_directory.clone(),
                source,
            })?;
        for profile in profiles {
            let profile = profile.map_err(|source| LauncherError::ReadDirectory {
                path: webview_directory.clone(),
                source,
            })?;
            let history = profile.path().join(STUDIO_BROWSER_HISTORY_FILENAME);
            if history.is_file() {
                candidates.push(history);
            }
        }
    }
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    Ok(candidates.pop())
}

fn read_studio_auth_history(
    history_path: &Path,
    minimum_visit_time: i64,
) -> Result<Option<(String, i64)>, LauncherError> {
    let snapshot_directory = env::temp_dir().join(format!(
        "roblox-studio-history-{}-{}",
        std::process::id(),
        chromium_timestamp_now()
    ));
    let result = (|| {
        fs::create_dir(&snapshot_directory).map_err(|source| {
            LauncherError::ReadBrowserHistory {
                path: history_path.to_path_buf(),
                message: format!("could not create a temporary snapshot: {source}"),
            }
        })?;
        let snapshot_path = snapshot_directory.join(STUDIO_BROWSER_HISTORY_FILENAME);
        fs::copy(history_path, &snapshot_path).map_err(|source| {
            LauncherError::ReadBrowserHistory {
                path: history_path.to_path_buf(),
                message: format!("could not copy the locked database: {source}"),
            }
        })?;
        for suffix in ["-journal", "-wal", "-shm"] {
            let sidecar = history_path
                .with_file_name(format!("{}{}", STUDIO_BROWSER_HISTORY_FILENAME, suffix));
            if sidecar.is_file() {
                let snapshot_sidecar = snapshot_directory
                    .join(format!("{}{}", STUDIO_BROWSER_HISTORY_FILENAME, suffix));
                let _ = fs::copy(sidecar, snapshot_sidecar);
            }
        }

        let connection =
            Connection::open_with_flags(&snapshot_path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(
                |source| LauncherError::ReadBrowserHistory {
                    path: history_path.to_path_buf(),
                    message: source.to_string(),
                },
            )?;
        let mut statement = connection
            .prepare(
                "SELECT url, last_visit_time FROM urls \
                 WHERE last_visit_time > ?1 \
                   AND url LIKE 'https://apis.roblox.com/oauth/v1/authorize%' \
                 ORDER BY last_visit_time DESC LIMIT 1",
            )
            .map_err(|source| LauncherError::ReadBrowserHistory {
                path: history_path.to_path_buf(),
                message: source.to_string(),
            })?;
        statement
            .query_row([minimum_visit_time], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .map_err(|source| LauncherError::ReadBrowserHistory {
                path: history_path.to_path_buf(),
                message: source.to_string(),
            })
    })();
    let _ = fs::remove_dir_all(&snapshot_directory);
    result
}

fn open_browser_url(url: &str) -> Result<(), LauncherError> {
    if !url.starts_with(STUDIO_AUTHORIZATION_URL_PREFIX) {
        return Err(LauncherError::BrowserOpenFailed {
            exit_code: "the URL was not a Roblox Studio authorization URL".to_owned(),
        });
    }

    let mut command = if env::var_os("FLATPAK_ID").is_some() {
        let mut command = Command::new("/usr/bin/flatpak-spawn");
        command.args(["--host", "xdg-open"]);
        command
    } else {
        Command::new("xdg-open")
    };
    let status = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| LauncherError::OpenBrowser { source })?;
    if !status.success() {
        return Err(LauncherError::BrowserOpenFailed {
            exit_code: status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        });
    }
    Ok(())
}

fn set_studio_client_boolean(
    settings: &mut Value,
    key: &str,
    value: bool,
    settings_path: &Path,
) -> Result<bool, LauncherError> {
    let root =
        settings
            .as_object_mut()
            .ok_or_else(|| LauncherError::InvalidStudioClientSettings {
                path: settings_path.to_path_buf(),
                message: "the root must be a JSON object".to_owned(),
            })?;
    let previous = root.insert(key.to_owned(), Value::Bool(value));
    Ok(previous != Some(Value::Bool(value)))
}

fn ensure_webview2_runtime(
    plan: StudioRuntimePlan,
    wine_binary: &Path,
    wine_prefix: &Path,
) -> Result<i32, LauncherError> {
    let runtime_directory = wine_prefix
        .join("drive_c")
        .join("Program Files (x86)")
        .join("Microsoft")
        .join("EdgeWebView")
        .join("Application");
    let runtime_version_directory = match find_webview2_runtime_directory(&runtime_directory)? {
        Some(path) => {
            tracing::debug!(
                path = %path.display(),
                "WebView2 runtime is already installed"
            );
            path
        }
        None => {
            let exit_code = uninstall_incompatible_webview2_runtimes(
                plan,
                wine_binary,
                wine_prefix,
                &runtime_directory,
            )?;
            if exit_code != 0 {
                return Ok(exit_code);
            }
            let installer = ensure_pinned_webview2_installer(wine_prefix)?;
            tracing::info!(
                version = plan.webview2_version(),
                "Installing the reference-tested WebView2 runtime"
            );
            let exit_code = run_wine(
                wine_binary,
                wine_prefix,
                &[
                    installer.display().to_string(),
                    "--msedgewebview".to_owned(),
                    "--do-not-launch-msedge".to_owned(),
                    "--system-level".to_owned(),
                ],
            )?;
            if exit_code != 0 {
                tracing::error!(exit_code, "WebView2 installer exited unsuccessfully");
                return Ok(exit_code);
            }
            find_webview2_runtime_directory(&runtime_directory)?.ok_or_else(|| {
                LauncherError::MissingWebView2Runtime {
                    path: runtime_directory.clone(),
                }
            })?
        }
    };

    configure_webview2_registration(plan, wine_binary, wine_prefix, &runtime_version_directory)
}

fn configure_webview2_registration(
    plan: StudioRuntimePlan,
    wine_binary: &Path,
    wine_prefix: &Path,
    runtime_version_directory: &Path,
) -> Result<i32, LauncherError> {
    let runtime_path = to_windows_drive_path(wine_prefix, runtime_version_directory)?;
    let version = runtime_version_directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
            path: runtime_version_directory.to_path_buf(),
        })?;
    if version != plan.webview2_version() {
        return Err(LauncherError::PrepareWebView2Runtime {
            message: format!(
                "selected version {version}, expected {}",
                plan.webview2_version(),
            ),
        });
    }
    let client_state_key =
        format!(r"HKCU\Software\Microsoft\EdgeUpdate\ClientState\{WEBVIEW2_STABLE_CLIENT_GUID}");
    let clients_key =
        format!(r"HKCU\Software\Microsoft\EdgeUpdate\Clients\{WEBVIEW2_STABLE_CLIENT_GUID}");

    let registrations = [
        (client_state_key, "EBWebView", runtime_path),
        (clients_key, "pv", version.to_owned()),
    ];
    for (registry_key, value_name, value) in registrations {
        let exit_code = run_wine(
            wine_binary,
            wine_prefix,
            &[
                "reg.exe".to_owned(),
                "ADD".to_owned(),
                registry_key,
                "/v".to_owned(),
                value_name.to_owned(),
                "/t".to_owned(),
                "REG_SZ".to_owned(),
                "/d".to_owned(),
                value,
                "/f".to_owned(),
            ],
        )?;
        if exit_code != 0 {
            tracing::error!(exit_code, value_name, "WebView2 registry repair failed");
            return Ok(exit_code);
        }
    }
    tracing::debug!(version, "Registered the installed WebView2 runtime");
    Ok(0)
}

#[derive(Debug, Deserialize)]
struct WebView2Download {
    #[serde(rename = "Url")]
    url: String,
    #[serde(rename = "FileId")]
    file_id: String,
    #[serde(rename = "SizeInBytes")]
    size_in_bytes: u64,
    #[serde(rename = "Hashes")]
    hashes: WebView2DownloadHashes,
}

#[derive(Debug, Deserialize)]
struct WebView2DownloadHashes {
    #[serde(rename = "Sha256")]
    sha256: String,
}

fn ensure_pinned_webview2_installer(wine_prefix: &Path) -> Result<PathBuf, LauncherError> {
    let cache_directory = wine_prefix.join("deployment-cache").join("webview2");
    fs::create_dir_all(&cache_directory).map_err(|source| LauncherError::WriteWebView2File {
        path: cache_directory.clone(),
        source,
    })?;
    let installer = cache_directory.join(PINNED_WEBVIEW2_INSTALLER);
    if installer.is_file() {
        match verify_pinned_webview2_installer(&installer) {
            Ok(()) => return Ok(installer),
            Err(error) => {
                tracing::warn!(
                    path = %installer.display(),
                    error = %error,
                    "Discarding an invalid cached WebView2 installer"
                );
                fs::remove_file(&installer).map_err(|source| LauncherError::WriteWebView2File {
                    path: installer.clone(),
                    source,
                })?;
            }
        }
    }

    let certificate = write_microsoft_root_certificate(&cache_directory)?;
    let download = fetch_pinned_webview2_download(&certificate)?;
    validate_pinned_webview2_download(&download)?;
    download_pinned_webview2_installer(&download, &installer)?;
    verify_pinned_webview2_installer(&installer)?;
    Ok(installer)
}

fn write_microsoft_root_certificate(cache_directory: &Path) -> Result<PathBuf, LauncherError> {
    let certificate = cache_directory.join("microsoft-root-2011.pem");
    let current_contents = match fs::read_to_string(&certificate) {
        Ok(contents) => Some(contents),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(LauncherError::ReadWebView2File {
                path: certificate,
                source,
            });
        }
    };
    if current_contents.as_deref() != Some(MICROSOFT_ROOT_CERTIFICATE) {
        fs::write(&certificate, MICROSOFT_ROOT_CERTIFICATE).map_err(|source| {
            LauncherError::WriteWebView2File {
                path: certificate.clone(),
                source,
            }
        })?;
    }
    Ok(certificate)
}

fn fetch_pinned_webview2_download(certificate: &Path) -> Result<WebView2Download, LauncherError> {
    let endpoint = format!(
        "{WEBVIEW2_DOWNLOAD_API}/versions/{PINNED_WEBVIEW2_VERSION}/files?action=GenerateDownloadInfo"
    );
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--request", "POST"])
        .arg("--cacert")
        .arg(certificate)
        .arg(endpoint)
        .output()
        .map_err(|source| LauncherError::RunWebView2Download { source })?;
    if !output.status.success() {
        return Err(LauncherError::WebView2DownloadFailed {
            exit_code: output
                .status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        });
    }
    let downloads = serde_json::from_slice::<Vec<WebView2Download>>(&output.stdout)
        .map_err(|source| LauncherError::ParseWebView2Download { source })?;
    select_pinned_webview2_download(downloads)
}

fn select_pinned_webview2_download(
    downloads: Vec<WebView2Download>,
) -> Result<WebView2Download, LauncherError> {
    downloads
        .into_iter()
        .find(|download| download.file_id == PINNED_WEBVIEW2_INSTALLER)
        .ok_or_else(|| LauncherError::PrepareWebView2Runtime {
            message: format!("Microsoft did not return {PINNED_WEBVIEW2_INSTALLER}"),
        })
}

fn validate_pinned_webview2_download(download: &WebView2Download) -> Result<Url, LauncherError> {
    if download.size_in_bytes != PINNED_WEBVIEW2_SIZE {
        return Err(LauncherError::PrepareWebView2Runtime {
            message: format!(
                "download size was {}, expected {PINNED_WEBVIEW2_SIZE}",
                download.size_in_bytes,
            ),
        });
    }
    if download.hashes.sha256 != PINNED_WEBVIEW2_SHA256_BASE64 {
        return Err(LauncherError::PrepareWebView2Runtime {
            message: "Microsoft returned an unexpected installer checksum".to_owned(),
        });
    }
    let url =
        Url::parse(&download.url).map_err(|source| LauncherError::PrepareWebView2Runtime {
            message: format!("Microsoft returned an invalid download URL: {source}"),
        })?;
    let host = url.host_str().unwrap_or_default();
    let trusted_https_host =
        url.scheme() == "https" && (host == "microsoft.com" || host.ends_with(".microsoft.com"));
    // Microsoft's content API currently signs this exact CDN family with an HTTP URL. The
    // installer is still safe to execute because its file name, byte size, and SHA-256 digest
    // are pinned above and verified again after the download.
    let trusted_signed_cdn = url.scheme() == "http"
        && (host == "dl.delivery.mp.microsoft.com"
            || host.ends_with(".dl.delivery.mp.microsoft.com"));
    if !trusted_https_host && !trusted_signed_cdn {
        return Err(LauncherError::PrepareWebView2Runtime {
            message: "Microsoft returned a download outside its approved servers".to_owned(),
        });
    }
    Ok(url)
}

fn download_pinned_webview2_installer(
    download: &WebView2Download,
    installer: &Path,
) -> Result<(), LauncherError> {
    let url = validate_pinned_webview2_download(download)?;
    let partial_installer = installer.with_extension("exe.partial");
    if partial_installer.exists() {
        fs::remove_file(&partial_installer).map_err(|source| LauncherError::WriteWebView2File {
            path: partial_installer.clone(),
            source,
        })?;
    }
    tracing::info!(
        version = PINNED_WEBVIEW2_VERSION,
        size = PINNED_WEBVIEW2_SIZE,
        "Downloading the managed WebView2 runtime"
    );
    let result = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=https",
        ])
        .arg("--output")
        .arg(&partial_installer)
        .arg(url.as_str())
        .status()
        .map_err(|source| LauncherError::RunWebView2Download { source })?;
    if !result.success() {
        return Err(LauncherError::WebView2DownloadFailed {
            exit_code: result
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        });
    }
    verify_pinned_webview2_installer(&partial_installer)?;
    fs::rename(&partial_installer, installer).map_err(|source| LauncherError::WriteWebView2File {
        path: installer.to_path_buf(),
        source,
    })
}

fn verify_pinned_webview2_installer(installer: &Path) -> Result<(), LauncherError> {
    let metadata = fs::metadata(installer).map_err(|source| LauncherError::ReadWebView2File {
        path: installer.to_path_buf(),
        source,
    })?;
    if metadata.len() != PINNED_WEBVIEW2_SIZE {
        return Err(LauncherError::PrepareWebView2Runtime {
            message: format!(
                "{} has size {}, expected {PINNED_WEBVIEW2_SIZE}",
                installer.display(),
                metadata.len(),
            ),
        });
    }
    let mut file = File::open(installer).map_err(|source| LauncherError::ReadWebView2File {
        path: installer.to_path_buf(),
        source,
    })?;
    let mut digest = DigestContext::new(&SHA256);
    let mut buffer = vec![0_u8; WEBVIEW2_DIGEST_BUFFER_SIZE];
    loop {
        let bytes_read =
            file.read(&mut buffer)
                .map_err(|source| LauncherError::ReadWebView2File {
                    path: installer.to_path_buf(),
                    source,
                })?;
        if bytes_read == 0 {
            break;
        }
        digest.update(&buffer[..bytes_read]);
    }
    if digest.finish().as_ref() != PINNED_WEBVIEW2_SHA256 {
        return Err(LauncherError::PrepareWebView2Runtime {
            message: format!("{} failed checksum verification", installer.display()),
        });
    }
    Ok(())
}

fn uninstall_incompatible_webview2_runtimes(
    plan: StudioRuntimePlan,
    wine_binary: &Path,
    wine_prefix: &Path,
    runtime_directory: &Path,
) -> Result<i32, LauncherError> {
    if !runtime_directory.is_dir() {
        return Ok(0);
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
        let runtime = entry.path();
        let version = runtime.file_name().and_then(|name| name.to_str());
        if !runtime.is_dir()
            || !runtime.join("msedgewebview2.exe").is_file()
            || version == Some(plan.webview2_version())
        {
            continue;
        }
        let uninstaller = runtime.join("Installer").join("setup.exe");
        if !uninstaller.is_file() {
            return Err(LauncherError::PrepareWebView2Runtime {
                message: format!("{} has no WebView2 uninstaller", runtime.display()),
            });
        }
        tracing::info!(
            current = version.unwrap_or("unknown"),
            next = plan.webview2_version(),
            "Replacing an incompatible WebView2 runtime"
        );
        let exit_code = run_wine(
            wine_binary,
            wine_prefix,
            &[
                uninstaller.display().to_string(),
                "--msedgewebview".to_owned(),
                "--uninstall".to_owned(),
                "--system-level".to_owned(),
                "--force-uninstall".to_owned(),
            ],
        )?;
        if runtime.is_dir() {
            return Ok(if exit_code == 0 { 1 } else { exit_code });
        }
    }
    Ok(0)
}

pub(crate) fn prepare_studio_runtime(
    plan: StudioRuntimePlan,
    wine_binary: &Path,
    wine_prefix: &Path,
    studio_executable: &Path,
) -> Result<i32, LauncherError> {
    ensure_managed_dxvk(studio_executable)?;
    let exit_code = ensure_webview2_runtime(plan, wine_binary, wine_prefix)?;
    if exit_code != 0 {
        return Ok(exit_code);
    }
    if let Err(error) = set_studio_webview2_override(studio_executable, true) {
        tracing::warn!(
            error = %error,
            "Could not enable Studio's managed WebView2 login runtime"
        );
    }
    configure_webview2_runtime(plan, wine_binary, wine_prefix)
}

fn ensure_managed_dxvk(studio_executable: &Path) -> Result<(), LauncherError> {
    let source_directory = Path::new(MANAGED_DXVK_DIRECTORY);
    if !source_directory.is_dir() {
        if env::var_os("FLATPAK_ID").is_some() {
            return Err(LauncherError::MissingManagedDxvk {
                path: source_directory.to_path_buf(),
            });
        }
        return Ok(());
    }

    let studio_directory =
        studio_executable
            .parent()
            .ok_or_else(|| LauncherError::InvalidStudioLaunchPath {
                path: studio_executable.to_path_buf(),
            })?;
    install_dxvk_files(source_directory, studio_directory)
}

fn install_dxvk_files(
    source_directory: &Path,
    studio_directory: &Path,
) -> Result<(), LauncherError> {
    for dll in MANAGED_DXVK_DLLS {
        let source = source_directory.join(dll);
        if !source.is_file() {
            return Err(LauncherError::MissingManagedDxvk { path: source });
        }
        let destination = studio_directory.join(dll);
        let matches = match fs::read(&destination) {
            Ok(installed) => {
                let managed = fs::read(&source).map_err(|source_error| {
                    LauncherError::PrepareManagedDxvkFile {
                        path: source.clone(),
                        source: source_error,
                    }
                })?;
                installed == managed
            }
            Err(source_error) if source_error.kind() == std::io::ErrorKind::NotFound => false,
            Err(source_error) => {
                return Err(LauncherError::PrepareManagedDxvkFile {
                    path: destination,
                    source: source_error,
                });
            }
        };
        if matches {
            continue;
        }

        let temporary = destination.with_extension(format!("dll.launcher-{}", std::process::id()));
        fs::copy(&source, &temporary).map_err(|source_error| {
            LauncherError::PrepareManagedDxvkFile {
                path: temporary.clone(),
                source: source_error,
            }
        })?;
        fs::rename(&temporary, &destination).map_err(|source_error| {
            LauncherError::PrepareManagedDxvkFile {
                path: destination,
                source: source_error,
            }
        })?;
    }
    Ok(())
}

pub(crate) fn run_studio_auth(
    plan: StudioRuntimePlan,
    wine_binary: &Path,
    wine_prefix: &Path,
    studio_executable: &Path,
    arguments: &[String],
) -> Result<i32, LauncherError> {
    let (mut command, program) = create_wine_command(wine_binary, wine_prefix, &[])?;
    command.arg(studio_executable);
    command.args(arguments);
    configure_studio_environment(&mut command, plan);
    spawn_wine_command(command, program)
}

pub(crate) fn create_wine_command(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
) -> Result<(Command, String), LauncherError> {
    let graphics_driver = wine_graphics_driver_for_display(environment_has_value("DISPLAY"));
    let exit_code = configure_wine_graphics_driver(wine_binary, wine_prefix, graphics_driver)?;
    if exit_code != 0 {
        return Err(LauncherError::WineGraphicsConfigurationFailed { exit_code });
    }
    create_unprepared_wine_command(wine_binary, wine_prefix, arguments)
}

fn create_unprepared_wine_command(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
) -> Result<(Command, String), LauncherError> {
    let configured_session_type = env::var_os("XDG_SESSION_TYPE");
    let session_type = wine_session_type(
        configured_session_type.as_deref(),
        environment_has_value("DISPLAY"),
        environment_has_value("WAYLAND_DISPLAY"),
    );
    create_wine_command_for_session_type(
        wine_binary,
        wine_prefix,
        arguments,
        session_type.as_deref(),
    )
}

fn create_wine_command_for_session_type(
    wine_binary: &Path,
    wine_prefix: &Path,
    arguments: &[String],
    session_type: Option<&OsStr>,
) -> Result<(Command, String), LauncherError> {
    fs::create_dir_all(wine_prefix).map_err(|source| LauncherError::CreateWinePrefix {
        path: wine_prefix.to_path_buf(),
        source,
    })?;

    let program = wine_binary.display().to_string();
    let mut command = Command::new(wine_binary);
    command.env("WINEPREFIX", wine_prefix);
    if let Some(session_type) = session_type {
        command.env("XDG_SESSION_TYPE", session_type);
    }
    command.args(arguments);
    Ok((command, program))
}

fn environment_has_value(variable_name: &str) -> bool {
    env::var_os(variable_name).is_some_and(|value| !value.is_empty())
}

fn wine_session_type(
    configured_session_type: Option<&OsStr>,
    x11_available: bool,
    wayland_available: bool,
) -> Option<OsString> {
    if let Some(session_type) = configured_session_type.filter(|value| !value.is_empty()) {
        return Some(session_type.to_os_string());
    }
    if wayland_available {
        return Some(OsString::from("wayland"));
    }
    x11_available.then(|| OsString::from("x11"))
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

fn spawn_wine_command(mut command: Command, program: String) -> Result<i32, LauncherError> {
    // Studio is a desktop application, not a command whose exit code the
    // launcher needs to wait for. Detaching all three streams is important:
    // otherwise a GUI caller's captured pipes stay open until Studio exits.
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // A launcher started from a terminal must not leave Studio in that
        // terminal's foreground process group. Otherwise closing the short-
        // lived launcher session can send Studio a hangup signal too.
        command.process_group(0);
    }
    let child = command.spawn().map_err(|source| LauncherError::RunWine {
        program: program.clone(),
        source,
    })?;
    tracing::info!(pid = child.id(), program = %program, "Started Studio process");
    Ok(0)
}

pub(crate) fn run_studio(
    plan: StudioRuntimePlan,
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
    configure_studio_environment(&mut command, plan);
    spawn_wine_command(command, program)
}

fn configure_studio_environment(command: &mut Command, plan: StudioRuntimePlan) {
    // These are the reference-tested Kombucha/WebView2 rendering settings.
    command.env(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        plan.browser_arguments(),
    );
    command.env("WINE_D3D_CONFIG", plan.wine_graphics_backend());
    let mut wine_dll_overrides = match env::var_os("WINEDLLOVERRIDES") {
        Some(value) => value,
        None => OsString::new(),
    };
    if !wine_dll_overrides.is_empty() {
        wine_dll_overrides.push(";");
    }
    wine_dll_overrides.push(plan.wine_dll_overrides());
    command.env("WINEDLLOVERRIDES", wine_dll_overrides);
    command.env("DXVK_LOG_LEVEL", "warn");
    command.env("DXVK_LOG_PATH", "none");
}

fn find_webview2_runtime_directory(
    runtime_directory: &Path,
) -> Result<Option<PathBuf>, LauncherError> {
    if !runtime_directory.is_dir() {
        return Ok(None);
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
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(PINNED_WEBVIEW2_VERSION)
            && path.is_dir()
            && path.join("msedgewebview2.exe").is_file()
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn to_windows_drive_path(wine_prefix: &Path, path: &Path) -> Result<String, LauncherError> {
    let wine_drive = wine_prefix.join("drive_c");
    let relative_path = match path.strip_prefix(&wine_drive) {
        Ok(relative_path) => relative_path,
        Err(source) => {
            return Err(LauncherError::StudioExecutableOutsideWineDrive {
                path: path.to_path_buf(),
                wine_drive,
                source,
            });
        }
    };

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
    if !value.is_empty() && !value.chars().any(char::is_whitespace) {
        return value.to_owned();
    }
    let mut quoted = String::with_capacity(value.len() + QUOTED_VALUE_DELIMITER_COUNT);
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

#[cfg(test)]
mod tests {
    use super::{
        configure_studio_environment, create_wine_command_for_session_type,
        find_webview2_runtime_directory, install_dxvk_files, quote_cmd_value,
        resolve_wine_binary_with, select_pinned_webview2_download,
        validate_pinned_webview2_download, wine_graphics_driver_for_display,
        wine_graphics_driver_needs_update, wine_graphics_preparation,
        wine_graphics_registry_arguments, wine_session_type, StudioRuntimePlan, WebView2Download,
        WineGraphicsPreparation, MANAGED_DXVK_DLLS,
    };
    use crate::config::StudioLoginMode;
    use behave::prelude::*;
    use std::ffi::OsStr;
    use std::fs;
    use std::process::Command;

    behave! {
        "Opening a place from the launcher" {
            "simple launch options" {
                setup {
                    let task_argument = quote_cmd_value("--task");
                    let edit_file_argument = quote_cmd_value("EditFile");
                }

                "stay readable to Studio" {
                    expect!(task_argument).to_equal("--task".to_owned())?;
                    expect!(edit_file_argument).to_equal("EditFile".to_owned())?;
                }
            }

            "a place path whose folder name contains spaces" {
                "is quoted for Studio" {
                    expect!(quote_cmd_value("C:\\Project Files\\place.rbxl"))
                        .to_equal("\"C:\\Project Files\\place.rbxl\"".to_owned())?;
                }
            }
        }

        "Choosing the Wine build for Studio" {
            "the launcher package contains its managed Wine build" {
                "uses that build instead of a second host installation" {
                    let managed_binary = std::env::current_exe()?;
                    expect!(resolve_wine_binary_with(
                        "wine",
                        &managed_binary,
                        None,
                    ))
                    .to_equal(Some(managed_binary))?;
                }
            }
        }

        "Starting the shared Wine session" {
            "XWayland is available on the Linux desktop" {
                setup {
                    let graphics_driver = wine_graphics_driver_for_display(true);
                    let registry_arguments = wine_graphics_registry_arguments(graphics_driver);
                    let current_registry = r#"
[Software\\Wine\\Drivers] 1787721000
"Graphics"="x11,wayland"
"#;
                    let restart_required = wine_graphics_driver_needs_update(
                        Some(current_registry),
                        graphics_driver,
                    );
                }

                "records X11 as the first driver for every Wine process" {
                    expect!(graphics_driver).to_equal("x11,wayland")?;
                    expect!(registry_arguments).to_equal(vec![
                        "reg.exe".to_owned(),
                        "ADD".to_owned(),
                        r"HKCU\Software\Wine\Drivers".to_owned(),
                        "/v".to_owned(),
                        "Graphics".to_owned(),
                        "/d".to_owned(),
                        "x11,wayland".to_owned(),
                        "/f".to_owned(),
                    ])?;
                    expect!(restart_required).to_be_false()?;
                }
            }

            "the desktop has no X11 display" {
                "keeps Wine's Wayland driver available" {
                    expect!(wine_graphics_driver_for_display(false)).to_equal("wayland")?;
                }
            }

            "an older prefix has no saved graphics driver" {
                setup {
                    let needs_update = wine_graphics_driver_needs_update(
                        Some("WINE REGISTRY Version 2\n"),
                        "x11,wayland",
                    );
                }

                "restarts Wine once after recording the driver when the prefix is cold" {
                    expect!(wine_graphics_preparation(needs_update, false))
                        .to_equal(WineGraphicsPreparation::ConfigureAndRestart)?;
                }

                "does not kill an already-running Studio session" {
                    expect!(wine_graphics_driver_needs_update(
                        Some("WINE REGISTRY Version 2\n"),
                        "x11,wayland",
                    ))
                    .to_be_true()?;
                    expect!(wine_graphics_preparation(needs_update, true))
                        .to_equal(WineGraphicsPreparation::ActiveSessionConflict)?;
                }
            }

            "Flatpak enter omits the desktop session type" {
                setup {
                    let inferred_session_type = wine_session_type(None, true, true);
                    let wine_binary = std::env::current_exe()?;
                    let wine_prefix = std::env::temp_dir().join(format!(
                        "roblox-studio-wine-session-{}",
                        std::process::id(),
                    ));
                    let (wine_command, _) = create_wine_command_for_session_type(
                        &wine_binary,
                        &wine_prefix,
                        &[],
                        inferred_session_type.as_deref(),
                    )?;
                    let command_session_type = wine_command
                        .get_envs()
                        .find(|(name, _)| *name == OsStr::new("XDG_SESSION_TYPE"))
                        .map(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()));
                    let wayland_override = wine_command
                        .get_envs()
                        .find(|(name, _)| *name == OsStr::new("WAYLAND_DISPLAY"));
                    fs::remove_dir_all(wine_prefix)?;
                }

                "reconstructs it without changing either display socket" {
                    expect!(inferred_session_type).to_equal(Some("wayland".into()))?;
                    expect!(command_session_type).to_equal(Some(Some("wayland".to_owned())))?;
                    expect!(wayland_override.is_none()).to_be_true()?;
                }
            }
        }

        "Preparing Studio's managed graphics layer" {
            "a newly installed Studio version" {
                setup {
                    let root = std::env::temp_dir().join(format!(
                        "roblox-studio-dxvk-install-{}",
                        std::process::id(),
                    ));
                    let source_directory = root.join("managed-dxvk");
                    let studio_directory = root.join("studio-version");
                    fs::create_dir_all(&source_directory)?;
                    fs::create_dir_all(&studio_directory)?;
                    for dll in MANAGED_DXVK_DLLS {
                        fs::write(source_directory.join(dll), b"managed DXVK")?;
                    }
                    fs::write(studio_directory.join("d3d11.dll"), b"old graphics DLL")?;
                }

                "receives the complete pinned DXVK set" {
                    install_dxvk_files(&source_directory, &studio_directory)?;
                    for dll in MANAGED_DXVK_DLLS {
                        expect!(fs::read(studio_directory.join(dll))?)
                            .to_equal(b"managed DXVK".to_vec())?;
                    }
                    fs::remove_dir_all(root)?;
                }
            }
        }

        "Rendering Studio's embedded login page" {
            "the WebView2 process compatibility profile" {
                "uses the Windows version that avoids unsupported DirectComposition" {
                    let plan = StudioRuntimePlan::new(StudioLoginMode::EmbeddedWebView);
                    expect!(plan.windows_version())
                        .to_equal("win8")?;
                }
            }

            "a Studio process prepared for the embedded login page" {
                setup {
                    let mut studio_command = Command::new("wine");
                    let plan = StudioRuntimePlan::new(StudioLoginMode::EmbeddedWebView);
                    configure_studio_environment(&mut studio_command, plan);
                    let browser_arguments = studio_command
                        .get_envs()
                        .find(|(name, _)| *name == OsStr::new(
                            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                        ))
                        .and_then(|(_, value)| value)
                        .and_then(OsStr::to_str)
                        .map(str::to_owned);
                    let wine_graphics_backend = studio_command
                        .get_envs()
                        .find(|(name, _)| *name == OsStr::new("WINE_D3D_CONFIG"))
                        .and_then(|(_, value)| value)
                        .and_then(OsStr::to_str)
                        .map(str::to_owned);
                    let wine_dll_overrides = studio_command
                        .get_envs()
                        .find(|(name, _)| *name == OsStr::new("WINEDLLOVERRIDES"))
                        .and_then(|(_, value)| value)
                        .and_then(OsStr::to_str)
                        .map(str::to_owned);
                }

                "uses a renderer that paints without Wine's hanging D3D11 WARP path" {
                    expect!(browser_arguments)
                        .to_equal(Some("--use-angle=swiftshader".to_owned()))?;
                    expect!(wine_graphics_backend)
                        .to_equal(Some("renderer=vulkan".to_owned()))?;
                    expect!(wine_dll_overrides.as_deref().is_some_and(|value| {
                        value.ends_with(
                            "d3d9,d3d10core,d3d11,dxgi=n,b;dxdiagn,winemenubuilder.exe,mscoree,mshtml="
                        )
                    })).to_be_true()?;
                }
            }

            "newer incompatible WebView2 files are also present" {
                setup {
                    let runtime_root = std::env::temp_dir().join(format!(
                        "roblox-studio-webview-selection-{}",
                        std::process::id(),
                    ));
                    let pinned_runtime = runtime_root.join("144.0.3719.92");
                    let newer_runtime = runtime_root.join("151.0.4129.101");
                    fs::create_dir_all(&pinned_runtime)?;
                    fs::create_dir_all(&newer_runtime)?;
                    fs::write(pinned_runtime.join("msedgewebview2.exe"), [])?;
                    fs::write(newer_runtime.join("msedgewebview2.exe"), [])?;
                    let selected_runtime = find_webview2_runtime_directory(&runtime_root)?;
                    fs::remove_dir_all(&runtime_root)?;
                }

                "selects the reference-tested runtime exactly" {
                    expect!(selected_runtime)
                        .to_equal(Some(runtime_root.join("144.0.3719.92")))?;
                }
            }

            "Microsoft returns the pinned runtime metadata" {
                setup {
                    let downloads = serde_json::from_str::<Vec<WebView2Download>>(r#"[
                        {
                            "Url": "https://download.microsoft.com/webview.exe",
                            "FileId": "MicrosoftEdge_X64_144.0.3719.92.exe",
                            "SizeInBytes": 185153080,
                            "Hashes": {
                                "Sha256": "dNC7zSOWrDfonkfozjelmPuzIGq9i8b7H33iSz/pJM0="
                            }
                        }
                    ]"#)?;
                    let download = select_pinned_webview2_download(downloads)?;
                }

                "accepts only the expected file, size, checksum, and approved host" {
                    expect!(validate_pinned_webview2_download(&download).is_ok())
                        .to_be_true()?;
                }
            }

            "Microsoft signs the pinned runtime with its HTTP delivery host" {
                setup {
                    let downloads = serde_json::from_str::<Vec<WebView2Download>>(r#"[
                        {
                            "Url": "http://msedge.b.tlu.dl.delivery.mp.microsoft.com/webview.exe",
                            "FileId": "MicrosoftEdge_X64_144.0.3719.92.exe",
                            "SizeInBytes": 185153080,
                            "Hashes": {
                                "Sha256": "dNC7zSOWrDfonkfozjelmPuzIGq9i8b7H33iSz/pJM0="
                            }
                        }
                    ]"#)?;
                    let download = select_pinned_webview2_download(downloads)?;
                }

                "accepts the signed host because the exact SHA-256 is pinned" {
                    expect!(validate_pinned_webview2_download(&download).is_ok())
                        .to_be_true()?;
                }
            }

            "an untrusted HTTP mirror copies the pinned metadata" {
                setup {
                    let downloads = serde_json::from_str::<Vec<WebView2Download>>(r#"[
                        {
                            "Url": "http://example.com/webview.exe",
                            "FileId": "MicrosoftEdge_X64_144.0.3719.92.exe",
                            "SizeInBytes": 185153080,
                            "Hashes": {
                                "Sha256": "dNC7zSOWrDfonkfozjelmPuzIGq9i8b7H33iSz/pJM0="
                            }
                        }
                    ]"#)?;
                    let download = select_pinned_webview2_download(downloads)?;
                }

                "rejects the mirror before downloading it" {
                    expect!(validate_pinned_webview2_download(&download).is_err())
                        .to_be_true()?;
                }
            }
        }
    }
}
