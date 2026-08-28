use crate::config::LauncherConfig;
use crate::error::LauncherError;
use crate::runtime::{
    create_wine_command, discover_studio_installation, exec_wine_stdio, resolve_wine_binary,
    StudioInstallation,
};
use serde_json::{json, Map, Value};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ROBLOX_MCP_SERVER_NAME: &str = "Roblox_Studio";
const FLATPAK_APP_ID: &str = "io.github.checkpickerupper.RobloxStudioLinuxLauncher";
const MCP_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const MCP_TOOL_LIST_TIMEOUT: Duration = Duration::from_secs(5);
const MCP_STUDIO_ATTACH_RETRY_COUNT: usize = 33;
const MCP_STUDIO_ATTACH_RETRY_DELAY: Duration = Duration::from_millis(250);
const MCP_STDIO_ARGUMENT: &str = "--stdio";
const MCP_VERBOSE_ARGUMENT: &str = "--verbose";
const FLATPAK_ENTERED_ENVIRONMENT: &str = "ROBLOX_LAUNCHER_FLATPAK_ENTERED";
const FLATPAK_STATUS_PATH_ENVIRONMENT: &str = "ROBLOX_LAUNCHER_FLATPAK_STATUS_PATH";
const FLATPAK_SPAWN_PATH: &str = "/usr/bin/flatpak-spawn";
const FLATPAK_ENTER_RETRY_COUNT: usize = 20;
const FLATPAK_ENTER_RETRY_DELAY: Duration = Duration::from_millis(250);
const REQUIRED_TOOL_NAMES: [&str; 3] = [
    "list_roblox_studios",
    "get_studio_state",
    "search_game_tree",
];
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn serve_mcp(launcher_config: &LauncherConfig) -> Result<i32, LauncherError> {
    if should_enter_running_flatpak_instance() {
        return run_mcp_in_running_flatpak_instance(launcher_config, "serve");
    }
    serve_mcp_direct(launcher_config)
}

fn serve_mcp_direct(launcher_config: &LauncherConfig) -> Result<i32, LauncherError> {
    let wine_binary = resolve_wine_binary(&launcher_config.wine_binary).ok_or_else(|| {
        LauncherError::McpRuntimeUnavailable {
            message: format!(
                "Wine command {:?} is unavailable; install Wine or configure --wine-binary",
                launcher_config.wine_binary
            ),
        }
    })?;
    let installation = discover_studio_installation(&wine_binary, &launcher_config.wine_prefix)?
        .ok_or_else(|| LauncherError::McpRuntimeUnavailable {
            message: "RobloxStudioBeta.exe was not found in the configured Wine prefix".to_owned(),
        })?;
    if !installation_is_inside_prefix(&installation) {
        return Err(LauncherError::McpRuntimeUnavailable {
            message: "the discovered Studio installation is outside the configured Wine prefix; MCP requires Studio and StudioMCP.exe to share one prefix".to_owned(),
        });
    }
    if !installation.mcp_executable.is_file() {
        return Err(LauncherError::MissingMcpExecutable {
            path: installation.mcp_executable,
        });
    }

    tracing::debug!(
        wine = %wine_binary.display(),
        prefix = %launcher_config.wine_prefix.display(),
        studio = %installation.studio_executable.display(),
        version = %installation.studio_version,
        mcp = %installation.mcp_executable.display(),
        "Starting the matching Roblox Studio MCP process"
    );
    exec_wine_stdio(
        &installation.wine_binary,
        &installation.wine_prefix,
        &installation.mcp_executable,
        &[MCP_STDIO_ARGUMENT.to_owned()],
    )
}

pub(crate) fn doctor_mcp(launcher_config: &LauncherConfig) -> Result<i32, LauncherError> {
    if should_enter_running_flatpak_instance() {
        return run_mcp_in_running_flatpak_instance(launcher_config, "doctor");
    }
    doctor_mcp_direct(launcher_config)
}

fn doctor_mcp_direct(launcher_config: &LauncherConfig) -> Result<i32, LauncherError> {
    let finding = inspect_mcp_connection(launcher_config)?;
    log_doctor_finding(&finding);
    Ok(finding.exit_code())
}

fn should_enter_running_flatpak_instance() -> bool {
    env::var_os("FLATPAK_ID").is_some() && env::var_os(FLATPAK_ENTERED_ENVIRONMENT).is_none()
}

pub(crate) fn route_auth_callback_if_needed(
    launcher_config: &LauncherConfig,
    studio_arguments: &[String],
) -> Result<Option<i32>, LauncherError> {
    if !should_enter_running_flatpak_instance() {
        return Ok(None);
    }

    let instance = wait_for_flatpak_studio_instance()?;
    let launcher =
        env::current_exe().map_err(|source| LauncherError::ResolveCurrentExecutable { source })?;
    let mut command = Command::new(FLATPAK_SPAWN_PATH);
    command.args(["--host", "flatpak", "enter"]);
    command.arg(&instance);
    command.arg("/usr/bin/env");
    append_flatpak_enter_environment(&mut command, None);
    command
        .arg(launcher)
        .arg("--config")
        .arg(absolute_path(&launcher_config.config_path))
        .arg("launch")
        .args(studio_arguments);

    tracing::debug!(
        instance,
        "Returning the browser login callback to the active Flatpak Studio sandbox"
    );
    let status = command
        .status()
        .map_err(|source| LauncherError::McpRuntimeUnavailable {
            message: format!(
                "could not enter the running Flatpak Studio sandbox for browser login: {source}"
            ),
        })?;
    match status.code() {
        Some(exit_code) => Ok(Some(exit_code)),
        None => Err(LauncherError::McpRuntimeUnavailable {
            message: "the Flatpak browser login callback exited without a status code".to_owned(),
        }),
    }
}

fn run_mcp_in_running_flatpak_instance(
    launcher_config: &LauncherConfig,
    action: &str,
) -> Result<i32, LauncherError> {
    let instance = wait_for_flatpak_studio_instance()?;
    let launcher =
        env::current_exe().map_err(|source| LauncherError::ResolveCurrentExecutable { source })?;
    let status_path =
        (action == "doctor").then(|| flatpak_status_path(&launcher_config.config_path));
    if let Some(path) = &status_path {
        let _ = fs::remove_file(path);
    }
    let mut command = Command::new(FLATPAK_SPAWN_PATH);
    command.args(["--host", "flatpak", "enter"]);
    command.arg(&instance);
    command.arg("/usr/bin/env");
    append_flatpak_enter_environment(&mut command, status_path.as_deref());
    command
        .arg(launcher)
        .arg("--config")
        .arg(absolute_path(&launcher_config.config_path))
        .arg("mcp")
        .arg(action);

    tracing::debug!(
        instance,
        action,
        "Running MCP inside the active Flatpak Studio sandbox"
    );
    let status = command
        .status()
        .map_err(|source| LauncherError::McpRuntimeUnavailable {
            message: format!("could not enter the running Flatpak Studio sandbox: {source}"),
        })?;
    if let Some(path) = status_path {
        return read_flatpak_status(path);
    }
    match status.code() {
        Some(exit_code) => Ok(exit_code),
        None => Err(LauncherError::McpRuntimeUnavailable {
            message: "the Flatpak MCP process exited without a status code".to_owned(),
        }),
    }
}

fn flatpak_status_path(config_path: &Path) -> PathBuf {
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".mcp-flatpak-status-{}", std::process::id()))
}

fn read_flatpak_status(path: PathBuf) -> Result<i32, LauncherError> {
    let contents =
        fs::read_to_string(&path).map_err(|source| LauncherError::McpRuntimeUnavailable {
            message: format!("the Flatpak MCP doctor did not report its result: {source}"),
        })?;
    let _ = fs::remove_file(&path);
    contents
        .trim()
        .parse::<i32>()
        .map_err(|error| LauncherError::McpRuntimeUnavailable {
            message: format!("the Flatpak MCP doctor reported an invalid exit code: {error}"),
        })
}

fn wait_for_flatpak_studio_instance() -> Result<String, LauncherError> {
    for attempt in 0..FLATPAK_ENTER_RETRY_COUNT {
        if let Some(instance) = find_flatpak_studio_instance()? {
            return Ok(instance);
        }
        if attempt + 1 < FLATPAK_ENTER_RETRY_COUNT {
            thread::sleep(FLATPAK_ENTER_RETRY_DELAY);
        }
    }
    Err(LauncherError::McpRuntimeUnavailable {
        message: "Flatpak MCP needs the launcher GUI running with Roblox Studio open; start the GUI, open a place, and try again".to_owned(),
    })
}

fn find_flatpak_studio_instance() -> Result<Option<String>, LauncherError> {
    let output = run_host_flatpak(["ps", "--columns=instance,application"])?;
    if !output.status.success() {
        return Err(flatpak_host_failure(
            "could not list running Flatpak instances",
            output,
        ));
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split_whitespace();
        let Some(instance) = fields.next() else {
            continue;
        };
        let Some(application) = fields.next() else {
            continue;
        };
        if application != FLATPAK_APP_ID {
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
        .map_err(|source| LauncherError::McpRuntimeUnavailable {
            message: format!("could not query the Flatpak host: {source}"),
        })
}

fn flatpak_host_failure(message: &str, output: Output) -> LauncherError {
    let details = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let message = if details.is_empty() {
        message.to_owned()
    } else {
        format!("{message}: {details}")
    };
    LauncherError::McpRuntimeUnavailable { message }
}

fn append_flatpak_enter_environment(command: &mut Command, status_path: Option<&Path>) {
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
            let mut assignment = std::ffi::OsString::from(key);
            assignment.push("=");
            assignment.push(value);
            command.arg(assignment);
        }
    }
    command.arg(format!("FLATPAK_ID={FLATPAK_APP_ID}"));
    command.arg(format!("{FLATPAK_ENTERED_ENVIRONMENT}=1"));
    if let Some(path) = status_path {
        command.arg(format!(
            "{FLATPAK_STATUS_PATH_ENVIRONMENT}={}",
            path.display()
        ));
    }
}

pub(crate) fn generate_client_configuration(
    launcher_config_path: &Path,
) -> Result<String, LauncherError> {
    let server_entry = client_server_entry(launcher_config_path)?;
    let mut servers = Map::new();
    servers.insert(ROBLOX_MCP_SERVER_NAME.to_owned(), server_entry);
    let mut root = Map::new();
    root.insert("mcpServers".to_owned(), Value::Object(servers));
    serialize_client_configuration(Value::Object(root))
}

pub(crate) fn setup_client_configuration(
    launcher_config_path: &Path,
    client_config_path: &Path,
    print_only: bool,
) -> Result<String, LauncherError> {
    let server_entry = client_server_entry(launcher_config_path)?;
    if print_only {
        return render_server_entry(server_entry);
    }

    let mut root = read_client_configuration(client_config_path)?;
    let servers = match root.get_mut("mcpServers") {
        Some(Value::Object(servers)) => servers,
        Some(_) => {
            return Err(LauncherError::InvalidMcpClientConfiguration {
                path: client_config_path.to_path_buf(),
                message: "mcpServers must be a JSON object".to_owned(),
            });
        }
        None => {
            root.insert("mcpServers".to_owned(), Value::Object(Map::new()));
            match root.get_mut("mcpServers") {
                Some(Value::Object(servers)) => servers,
                Some(_) | None => {
                    return Err(LauncherError::InvalidMcpClientConfiguration {
                        path: client_config_path.to_path_buf(),
                        message: "could not create the mcpServers JSON object".to_owned(),
                    });
                }
            }
        }
    };
    if let Some(existing_entry) = servers.get(ROBLOX_MCP_SERVER_NAME) {
        if !existing_entry.is_object() {
            return Err(LauncherError::InvalidMcpClientConfiguration {
                path: client_config_path.to_path_buf(),
                message: format!(
                    "the existing {ROBLOX_MCP_SERVER_NAME} entry must be a JSON object"
                ),
            });
        }
    }
    servers.insert(ROBLOX_MCP_SERVER_NAME.to_owned(), server_entry);
    let serialized = serialize_client_configuration(Value::Object(root))?;
    write_client_configuration(client_config_path, serialized.as_bytes())?;
    Ok(serialized)
}

fn inspect_mcp_connection(
    launcher_config: &LauncherConfig,
) -> Result<McpDoctorFinding, LauncherError> {
    let wine_binary = match resolve_wine_binary(&launcher_config.wine_binary) {
        Some(path) => path,
        None => return Ok(McpDoctorFinding::WineUnavailable),
    };
    let installation =
        match discover_studio_installation(&wine_binary, &launcher_config.wine_prefix)? {
            Some(installation) => installation,
            None => return Ok(McpDoctorFinding::StudioUnavailable),
        };
    if !installation_is_inside_prefix(&installation) {
        return Ok(McpDoctorFinding::ProcessUnavailable {
            message: "the discovered Studio installation is outside the configured Wine prefix; MCP requires one shared prefix".to_owned(),
        });
    }
    if !installation.mcp_executable.is_file() {
        return Ok(McpDoctorFinding::McpUnavailable {
            path: installation.mcp_executable.clone(),
        });
    }

    let mut probe = match McpProbe::start(&installation) {
        Ok(probe) => probe,
        Err(error) => {
            return Ok(McpDoctorFinding::ProcessUnavailable {
                message: error.to_string(),
            });
        }
    };
    let initialize_params = json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {
            "name": "RobloxStudioLinuxLauncher",
            "version": env!("CARGO_PKG_VERSION")
        }
    });
    let initialize = probe.request("initialize", initialize_params, MCP_PROBE_TIMEOUT)?;
    if initialize.get("protocolVersion").is_none() {
        return Err(LauncherError::McpProtocolFailure {
            method: "initialize".to_owned(),
            message: "the matching StudioMCP.exe returned no protocol version".to_owned(),
        });
    }
    probe.notify("notifications/initialized", json!({}))?;

    let studio_ids = wait_for_studio_ids(
        || probe.call_tool("list_roblox_studios", json!({}), MCP_PROBE_TIMEOUT),
        MCP_STUDIO_ATTACH_RETRY_COUNT,
        MCP_STUDIO_ATTACH_RETRY_DELAY,
    )?;
    match studio_ids.len() {
        0 => Ok(studio_session_finding(&launcher_config.wine_prefix)),
        count if count > 1 => Ok(McpDoctorFinding::MultipleStudioSessions { count }),
        _ => {
            let studio_id = match studio_ids.first() {
                Some(id) => id,
                None => {
                    return Err(LauncherError::McpProtocolFailure {
                        method: "list_roblox_studios".to_owned(),
                        message: "the response contained one studio but no id".to_owned(),
                    });
                }
            };
            let tools_result = probe.request("tools/list", json!({}), MCP_TOOL_LIST_TIMEOUT)?;
            let tool_names = extract_tool_names(&tools_result).ok_or_else(|| {
                LauncherError::McpProtocolFailure {
                    method: "tools/list".to_owned(),
                    message: "the response did not contain a tools array".to_owned(),
                }
            })?;
            for required_tool in REQUIRED_TOOL_NAMES {
                if !tool_names.iter().any(|name| name == required_tool) {
                    return Err(LauncherError::McpProtocolFailure {
                        method: "tools/list".to_owned(),
                        message: format!("required Studio tool {required_tool:?} is missing"),
                    });
                }
            }
            match probe.call_tool(
                "get_studio_state",
                json!({ "studio_id": studio_id }),
                MCP_PROBE_TIMEOUT,
            )? {
                ToolCallReply::Success(_) => {}
                ToolCallReply::Error(message) => {
                    return Err(LauncherError::McpProtocolFailure {
                        method: "tools/call:get_studio_state".to_owned(),
                        message,
                    });
                }
            }
            match probe.call_tool(
                "search_game_tree",
                json!({ "studio_id": studio_id }),
                MCP_PROBE_TIMEOUT,
            )? {
                ToolCallReply::Success(_) => {}
                ToolCallReply::Error(message) => {
                    return Err(LauncherError::McpProtocolFailure {
                        method: "tools/call:search_game_tree".to_owned(),
                        message,
                    });
                }
            }
            match assistant_restart_hint(&launcher_config.wine_prefix) {
                Some(path) => {
                    tracing::warn!(
                        path = %path.display(),
                        "Roblox Studio reported an Assistant plugin update; restart Studio before relying on MCP"
                    );
                    Ok(McpDoctorFinding::RestartStudio { path })
                }
                None => Ok(McpDoctorFinding::Connected),
            }
        }
    }
}

fn log_doctor_finding(finding: &McpDoctorFinding) {
    match finding {
        McpDoctorFinding::WineUnavailable => {
            tracing::error!("MCP cannot start because Wine is unavailable")
        }
        McpDoctorFinding::StudioUnavailable => {
            tracing::error!("MCP cannot start because Roblox Studio is not installed")
        }
        McpDoctorFinding::McpUnavailable { path } => tracing::error!(
            path = %path.display(),
            "StudioMCP.exe is missing from the selected Studio version; run install to repair it"
        ),
        McpDoctorFinding::ProcessUnavailable { message } => {
            tracing::error!(message = %message, "StudioMCP.exe could not be started")
        }
        McpDoctorFinding::StudioNotRunning => tracing::warn!(
            "StudioMCP is installed, but Roblox Studio is not running with an open place"
        ),
        McpDoctorFinding::StudioPlaceNotOpen => tracing::warn!(
            "Roblox Studio is running, but no open place was found; open a place before connecting MCP"
        ),
        McpDoctorFinding::StudioNeedsSignIn => tracing::warn!(
            "Roblox Studio is waiting for sign-in; complete sign-in before connecting MCP"
        ),
        McpDoctorFinding::StudioMcpNotEnabled => tracing::warn!(
            "Roblox Studio is running, but no MCP session is attached; enable Assistant > Manage MCP Servers > Enable Studio as MCP server"
        ),
        McpDoctorFinding::RestartStudio { path } => tracing::warn!(
            path = %path.display(),
            "Roblox Studio's Assistant plugin changed; restart Studio before connecting MCP"
        ),
        McpDoctorFinding::StudioSessionUnavailable => tracing::warn!(
            "StudioMCP is running, but it could not verify a connected Studio session"
        ),
        McpDoctorFinding::MultipleStudioSessions { count } => tracing::warn!(
            count,
            "Multiple Roblox Studio sessions are connected; select a studio_id before making tool calls"
        ),
        McpDoctorFinding::Connected => tracing::info!(
            "MCP is connected to one Roblox Studio place and passed state/tree tool checks"
        ),
    }
}

fn client_server_entry(launcher_config_path: &Path) -> Result<Value, LauncherError> {
    let launcher_path = absolute_path(launcher_config_path);
    let command = env::var_os("FLATPAK_ID").map_or_else(
        || env::current_exe().map_err(|source| LauncherError::ResolveCurrentExecutable { source }),
        |_| Ok(PathBuf::from("flatpak")),
    )?;
    let arguments = match env::var_os("FLATPAK_ID") {
        Some(_) => vec![
            "run".to_owned(),
            "--command=roblox-studio-linux-launcher".to_owned(),
            FLATPAK_APP_ID.to_owned(),
            "--config".to_owned(),
            launcher_path.display().to_string(),
            "mcp".to_owned(),
            "serve".to_owned(),
        ],
        None => vec![
            "--config".to_owned(),
            launcher_path.display().to_string(),
            "mcp".to_owned(),
            "serve".to_owned(),
        ],
    };
    Ok(json!({
        "command": command.display().to_string(),
        "args": arguments
    }))
}

fn render_server_entry(server_entry: Value) -> Result<String, LauncherError> {
    let mut servers = Map::new();
    servers.insert(ROBLOX_MCP_SERVER_NAME.to_owned(), server_entry);
    let mut root = Map::new();
    root.insert("mcpServers".to_owned(), Value::Object(servers));
    serialize_client_configuration(Value::Object(root))
}

fn serialize_client_configuration(configuration: Value) -> Result<String, LauncherError> {
    serde_json::to_string_pretty(&configuration)
        .map_err(|source| LauncherError::SerializeMcpClientConfiguration { source })
        .map(|mut text| {
            text.push('\n');
            text
        })
}

fn read_client_configuration(path: &Path) -> Result<Map<String, Value>, LauncherError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(source) => {
            return Err(LauncherError::ReadMcpClientConfiguration {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let parsed = serde_json::from_str::<Value>(&contents).map_err(|source| {
        LauncherError::ParseMcpClientConfiguration {
            path: path.to_path_buf(),
            source,
        }
    })?;
    match parsed {
        Value::Object(root) => Ok(root),
        _ => Err(LauncherError::InvalidMcpClientConfiguration {
            path: path.to_path_buf(),
            message: "the client configuration root must be a JSON object".to_owned(),
        }),
    }
}

fn write_client_configuration(path: &Path, contents: &[u8]) -> Result<(), LauncherError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| LauncherError::WriteMcpClientConfiguration {
        path: path.to_path_buf(),
        source,
    })?;
    if path.is_file() {
        let backup_path = next_backup_path(path);
        fs::copy(path, &backup_path).map_err(|source| {
            LauncherError::BackupMcpClientConfiguration {
                path: backup_path,
                source,
            }
        })?;
    }

    let stamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(_) => 0,
    };
    let temporary_file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map_or("mcp", |name| name);
    let temporary_path = parent.join(format!(
        ".{}.tmp-{}-{}",
        temporary_file_name,
        std::process::id(),
        stamp
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|source| LauncherError::WriteMcpClientConfiguration {
            path: path.to_path_buf(),
            source,
        })?;
    if let Err(source) = file.write_all(contents) {
        remove_temporary_file(&temporary_path);
        return Err(LauncherError::WriteMcpClientConfiguration {
            path: path.to_path_buf(),
            source,
        });
    }
    if let Err(source) = file.sync_all() {
        remove_temporary_file(&temporary_path);
        return Err(LauncherError::WriteMcpClientConfiguration {
            path: path.to_path_buf(),
            source,
        });
    }
    drop(file);
    if let Err(source) = fs::rename(&temporary_path, path) {
        remove_temporary_file(&temporary_path);
        return Err(LauncherError::WriteMcpClientConfiguration {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn remove_temporary_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) => {
            tracing::debug!(path = %path.display(), error = %error, "Could not remove temporary MCP configuration file")
        }
    }
}

fn next_backup_path(path: &Path) -> PathBuf {
    let first = PathBuf::from(format!("{}.bak", path.display()));
    if !first.exists() {
        return first;
    }
    let mut suffix = 1_u64;
    loop {
        let candidate = PathBuf::from(format!("{}.bak.{}", path.display(), suffix));
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match env::current_dir() {
        Ok(directory) => directory.join(path),
        Err(_) => path.to_path_buf(),
    }
}

fn installation_is_inside_prefix(installation: &StudioInstallation) -> bool {
    installation
        .studio_executable
        .starts_with(installation.wine_prefix.join("drive_c"))
        && installation
            .mcp_executable
            .starts_with(installation.wine_prefix.join("drive_c"))
}

struct McpProbe {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<ProtocolEvent>,
}

impl McpProbe {
    fn start(installation: &StudioInstallation) -> Result<Self, LauncherError> {
        let (mut command, _) =
            create_wine_command(&installation.wine_binary, &installation.wine_prefix, &[])?;
        command
            .arg(&installation.mcp_executable)
            .arg(MCP_STDIO_ARGUMENT)
            .arg(MCP_VERBOSE_ARGUMENT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|source| LauncherError::RunWine {
            program: installation.wine_binary.display().to_string(),
            source,
        })?;
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                stop_probe_process(&mut child);
                return Err(LauncherError::McpProtocolFailure {
                    method: "spawn".to_owned(),
                    message: "StudioMCP stdout was not available".to_owned(),
                });
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                stop_probe_process(&mut child);
                return Err(LauncherError::McpProtocolFailure {
                    method: "spawn".to_owned(),
                    message: "StudioMCP stderr was not available".to_owned(),
                });
            }
        };
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                stop_probe_process(&mut child);
                return Err(LauncherError::McpProtocolFailure {
                    method: "spawn".to_owned(),
                    message: "StudioMCP stdin was not available".to_owned(),
                });
            }
        };
        let (sender, events) = mpsc::channel();
        spawn_stdout_reader(stdout, sender);
        spawn_stderr_reader(stderr);
        tracing::debug!(
            studio = %installation.studio_executable.display(),
            mcp = %installation.mcp_executable.display(),
            "Started MCP probe process"
        );
        Ok(Self {
            child,
            stdin,
            events,
        })
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LauncherError> {
        let request_id = next_request_id();
        let message = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });
        write_stdio_message(&mut self.stdin, message)?;
        loop {
            match self.events.recv_timeout(timeout) {
                Ok(ProtocolEvent::Message(message)) => {
                    if message.get("id") == Some(&json!(request_id)) {
                        return protocol_result(message, method);
                    }
                }
                Ok(ProtocolEvent::Ended) => {
                    return Err(LauncherError::McpProtocolFailure {
                        method: method.to_owned(),
                        message: "StudioMCP closed its stdio stream".to_owned(),
                    });
                }
                Ok(ProtocolEvent::Failed(message)) => {
                    return Err(LauncherError::McpProtocolFailure {
                        method: method.to_owned(),
                        message,
                    });
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(LauncherError::McpProtocolTimeout {
                        method: method.to_owned(),
                        timeout_seconds: timeout.as_secs(),
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(LauncherError::McpProtocolFailure {
                        method: method.to_owned(),
                        message: "the StudioMCP reader stopped unexpectedly".to_owned(),
                    });
                }
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), LauncherError> {
        write_stdio_message(
            &mut self.stdin,
            json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params
            }),
        )
    }

    fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<ToolCallReply, LauncherError> {
        let result = self.request(
            "tools/call",
            json!({ "name": tool_name, "arguments": arguments }),
            timeout,
        )?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Ok(ToolCallReply::Error(extract_error_text(&result)));
        }
        Ok(ToolCallReply::Success(result))
    }
}

impl Drop for McpProbe {
    fn drop(&mut self) {
        stop_probe_process(&mut self.child);
    }
}

fn stop_probe_process(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(error) = child.kill() {
                tracing::debug!(error = %error, "Could not stop MCP probe process");
            }
            if let Err(error) = child.wait() {
                tracing::debug!(error = %error, "Could not collect MCP probe process");
            }
        }
        Err(error) => tracing::debug!(error = %error, "Could not inspect MCP probe process"),
    }
}

enum ProtocolEvent {
    Message(Value),
    Ended,
    Failed(String),
}

enum ToolCallReply {
    Success(Value),
    Error(String),
}

fn spawn_stdout_reader(stdout: impl Read + Send + 'static, sender: Sender<ProtocolEvent>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    send_protocol_event(&sender, ProtocolEvent::Ended);
                    break;
                }
                Ok(_) => match serde_json::from_str::<Value>(line.trim()) {
                    Ok(message) => send_protocol_event(&sender, ProtocolEvent::Message(message)),
                    Err(error) => send_protocol_event(
                        &sender,
                        ProtocolEvent::Failed(format!("invalid JSON from StudioMCP: {error}")),
                    ),
                },
                Err(error) => {
                    send_protocol_event(
                        &sender,
                        ProtocolEvent::Failed(format!("could not read StudioMCP stdout: {error}")),
                    );
                    break;
                }
            }
        }
    });
}

fn spawn_stderr_reader(stderr: impl Read + Send + 'static) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buffer = String::new();
        match reader.read_to_string(&mut buffer) {
            Ok(_) => {}
            Err(error) => tracing::debug!(error = %error, "Could not drain StudioMCP stderr"),
        }
    });
}

fn send_protocol_event(sender: &Sender<ProtocolEvent>, event: ProtocolEvent) {
    if let Err(error) = sender.send(event) {
        tracing::debug!(error = %error, "MCP probe event receiver stopped");
    }
}

fn write_stdio_message(stdin: &mut ChildStdin, message: Value) -> Result<(), LauncherError> {
    let serialized =
        serde_json::to_string(&message).map_err(|source| LauncherError::McpProtocolFailure {
            method: "write".to_owned(),
            message: source.to_string(),
        })?;
    stdin
        .write_all(serialized.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|source| LauncherError::McpProtocolFailure {
            method: "write".to_owned(),
            message: source.to_string(),
        })
}

fn protocol_result(message: Value, method: &str) -> Result<Value, LauncherError> {
    let object = match message {
        Value::Object(object) => object,
        _ => {
            return Err(LauncherError::McpProtocolFailure {
                method: method.to_owned(),
                message: "the response was not a JSON object".to_owned(),
            });
        }
    };
    if let Some(error) = object.get("error") {
        return Err(LauncherError::McpProtocolFailure {
            method: method.to_owned(),
            message: extract_error_text(error),
        });
    }
    match object.get("result") {
        Some(result) => Ok(result.clone()),
        None => Err(LauncherError::McpProtocolFailure {
            method: method.to_owned(),
            message: "the response contained neither result nor error".to_owned(),
        }),
    }
}

fn extract_error_text(value: &Value) -> String {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_owned();
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for item in content {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                return text.to_owned();
            }
        }
    }
    value.to_string()
}

fn extract_tool_document(result: &Value) -> Value {
    if let Some(structured) = result.get("structuredContent") {
        return structured.clone();
    }
    let Some(content) = result.get("content").and_then(Value::as_array) else {
        return result.clone();
    };
    for item in content {
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            continue;
        };
        if let Ok(document) = serde_json::from_str::<Value>(text) {
            return document;
        }
    }
    result.clone()
}

fn extract_studio_ids(result: &Value) -> Option<Vec<String>> {
    let document = extract_tool_document(result);
    let studios = document.get("studios")?.as_array()?;
    let mut ids = Vec::new();
    for studio in studios {
        let id = studio.get("id")?.as_str()?.to_owned();
        ids.push(id);
    }
    Some(ids)
}

fn wait_for_studio_ids<F>(
    mut list_studios: F,
    max_attempts: usize,
    retry_delay: Duration,
) -> Result<Vec<String>, LauncherError>
where
    F: FnMut() -> Result<ToolCallReply, LauncherError>,
{
    let max_attempts = max_attempts.max(1);
    let mut last_error = None;
    for attempt in 0..max_attempts {
        match list_studios()? {
            ToolCallReply::Success(result) => {
                let studio_ids = extract_studio_ids(&result).ok_or_else(|| {
                    LauncherError::McpProtocolFailure {
                        method: "list_roblox_studios".to_owned(),
                        message: "the response did not contain a studios array".to_owned(),
                    }
                })?;
                if !studio_ids.is_empty() {
                    return Ok(studio_ids);
                }
            }
            ToolCallReply::Error(error) => last_error = Some(error),
        }

        if attempt + 1 < max_attempts {
            thread::sleep(retry_delay);
        }
    }
    if let Some(error) = last_error {
        tracing::debug!(%error, "StudioMCP could not list Studio sessions");
    }
    Ok(Vec::new())
}

fn extract_tool_names(result: &Value) -> Option<Vec<String>> {
    let document = extract_tool_document(result);
    let tools = document.get("tools")?.as_array()?;
    let mut names = Vec::new();
    for tool in tools {
        let name = tool.get("name")?.as_str()?.to_owned();
        names.push(name);
    }
    Some(names)
}

fn next_request_id() -> u64 {
    NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

enum StudioProcessPresence {
    Running,
    NotRunning,
    Unknown,
}

enum StudioPlacePresence {
    Open,
    NotOpen,
    Unknown,
}

fn detect_studio_process() -> StudioProcessPresence {
    let output = match Command::new("pgrep")
        .args(["-af", "RobloxStudioBeta.exe"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return StudioProcessPresence::Unknown,
    };
    let has_studio_process = String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(is_studio_process_command_line);
    if output.status.success() && has_studio_process {
        StudioProcessPresence::Running
    } else if output.status.code() == Some(1) {
        StudioProcessPresence::NotRunning
    } else {
        StudioProcessPresence::Unknown
    }
}

fn is_studio_process_command_line(command_line: &str) -> bool {
    command_line.split_whitespace().any(|token| {
        let token = token.trim_matches('"');
        token == "RobloxStudioBeta.exe"
            || token.ends_with("/RobloxStudioBeta.exe")
            || token.ends_with("\\RobloxStudioBeta.exe")
    })
}

fn studio_session_finding(wine_prefix: &Path) -> McpDoctorFinding {
    match detect_studio_process() {
        StudioProcessPresence::Running => match assistant_restart_hint(wine_prefix) {
            Some(path) => McpDoctorFinding::RestartStudio { path },
            None if studio_requires_sign_in(wine_prefix) => McpDoctorFinding::StudioNeedsSignIn,
            None => match inspect_studio_place(wine_prefix) {
                StudioPlacePresence::Open => McpDoctorFinding::StudioMcpNotEnabled,
                StudioPlacePresence::NotOpen => McpDoctorFinding::StudioPlaceNotOpen,
                StudioPlacePresence::Unknown => McpDoctorFinding::StudioSessionUnavailable,
            },
        },
        StudioProcessPresence::NotRunning => McpDoctorFinding::StudioNotRunning,
        StudioProcessPresence::Unknown => McpDoctorFinding::StudioSessionUnavailable,
    }
}

fn inspect_studio_place(wine_prefix: &Path) -> StudioPlacePresence {
    let Some(log_path) = latest_studio_log(wine_prefix) else {
        return StudioPlacePresence::Unknown;
    };
    let contents = match fs::read_to_string(log_path) {
        Ok(contents) => contents,
        Err(_) => return StudioPlacePresence::Unknown,
    };
    // A current Studio log with no successful open-place marker represents the
    // start page (or another no-place state).  Keep Unknown only for cases
    // where the log cannot be read at all; otherwise the doctor would collapse
    // the user-actionable "open a place" state into a generic failure.
    let mut presence = StudioPlacePresence::NotOpen;
    for line in contents.lines() {
        if line.contains("OpenPlaceSuccess") || line.contains("OpenPlacePostLoadDataModelTasks") {
            presence = StudioPlacePresence::Open;
        }
        if line.contains("ClosePlaceEnd") || line.contains("State: ClosePlace") {
            presence = StudioPlacePresence::NotOpen;
        }
    }
    presence
}

fn studio_requires_sign_in(wine_prefix: &Path) -> bool {
    let Some(log_path) = latest_studio_log(wine_prefix) else {
        return false;
    };
    let contents = match fs::read_to_string(log_path) {
        Ok(contents) => contents,
        Err(_) => return false,
    };
    log_indicates_sign_in_required(&contents)
}

fn log_indicates_sign_in_required(contents: &str) -> bool {
    contents.contains("LoginDialog Error")
        || contents.contains("Embedded Web Browser fail to load")
        || contents.contains("show login dialog [start]")
}

fn latest_studio_log(wine_prefix: &Path) -> Option<PathBuf> {
    let users_directory = wine_prefix.join("drive_c").join("users");
    let user_entries = match fs::read_dir(&users_directory) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(
                path = %users_directory.display(),
                error = %error,
                "Could not inspect Wine users for a Studio log"
            );
            return None;
        }
    };
    let mut newest_log = None;
    for user_entry in user_entries {
        let user_entry = match user_entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let log_directory = user_entry
            .path()
            .join("AppData")
            .join("Local")
            .join("Roblox")
            .join("logs");
        let log_entries = match fs::read_dir(log_directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for log_entry in log_entries {
            let log_entry = match log_entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = log_entry.path();
            let is_studio_log = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("_Studio_") && name.ends_with("_last.log"));
            if !is_studio_log {
                continue;
            }
            let modified = match log_entry
                .metadata()
                .and_then(|metadata| metadata.modified())
            {
                Ok(modified) => modified,
                Err(_) => continue,
            };
            let replace = newest_log
                .as_ref()
                .is_none_or(|(newest_modified, _)| modified > *newest_modified);
            if replace {
                newest_log = Some((modified, path));
            }
        }
    }
    newest_log.map(|(_, path)| path)
}

fn assistant_restart_hint(wine_prefix: &Path) -> Option<PathBuf> {
    let path = latest_studio_log(wine_prefix)?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                "Could not inspect the latest Studio log for an Assistant update"
            );
            return None;
        }
    };
    if contents.contains("Assistant plugin version changed")
        && contents.contains("Please restart Roblox Studio")
    {
        Some(path)
    } else {
        None
    }
}

enum McpDoctorFinding {
    WineUnavailable,
    StudioUnavailable,
    McpUnavailable { path: PathBuf },
    ProcessUnavailable { message: String },
    StudioNotRunning,
    StudioPlaceNotOpen,
    StudioNeedsSignIn,
    StudioMcpNotEnabled,
    RestartStudio { path: PathBuf },
    StudioSessionUnavailable,
    MultipleStudioSessions { count: usize },
    Connected,
}

impl McpDoctorFinding {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Connected => 0,
            Self::WineUnavailable
            | Self::StudioUnavailable
            | Self::McpUnavailable { .. }
            | Self::ProcessUnavailable { .. }
            | Self::StudioNotRunning
            | Self::StudioPlaceNotOpen
            | Self::StudioNeedsSignIn
            | Self::StudioMcpNotEnabled
            | Self::RestartStudio { .. }
            | Self::StudioSessionUnavailable
            | Self::MultipleStudioSessions { .. } => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_studio_ids, extract_tool_names, log_indicates_sign_in_required,
        setup_client_configuration, wait_for_studio_ids, ToolCallReply,
    };
    use behave::prelude::*;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    behave! {
        "Reading the open Studio connection" {
            "keeps the Studio ID returned by the server" {
                let response = json!({
                    "content": [{
                        "type": "text",
                        "text": "{\"studios\":[{\"id\":\"studio-one\",\"name\":\"Test place\"}]}"
                    }]
                });
                expect!(extract_studio_ids(&response))
                    .to_equal(Some(vec!["studio-one".to_owned()]))?;
            }

            "recognizes the tools needed to inspect a place" {
                let response = json!({
                    "tools": [
                        {"name": "list_roblox_studios"},
                        {"name": "get_studio_state"},
                        {"name": "search_game_tree"}
                    ]
                });
                expect!(extract_tool_names(&response)).to_equal(Some(vec![
                    "list_roblox_studios".to_owned(),
                    "get_studio_state".to_owned(),
                    "search_game_tree".to_owned(),
                ]))?;
            }

            "reports an unusable response when the server omits its list" {
                expect!(extract_studio_ids(&json!({"content": []}))).to_be_none()?;
            }

            "waits for Studio's next connection attempt" {
                let mut replies = VecDeque::from([
                    ToolCallReply::Success(json!({
                        "content": [{
                            "type": "text",
                            "text": "{\"studios\":[]}"
                        }]
                    })),
                    ToolCallReply::Success(json!({
                        "content": [{
                            "type": "text",
                            "text": "{\"studios\":[{\"id\":\"studio-one\",\"name\":\"Test place\"}]}"
                        }]
                    })),
                ]);
                let mut calls = 0;
                let studio_ids = match wait_for_studio_ids(
                    || {
                        calls += 1;
                        match replies.pop_front() {
                            Some(reply) => Ok(reply),
                            None => Ok(ToolCallReply::Success(json!({
                                "content": [{"type": "text", "text": "{\"studios\":[]}"}]
                            }))),
                        }
                    },
                    2,
                    Duration::ZERO,
                ) {
                    Ok(studio_ids) => studio_ids,
                    Err(error) => {
                        expect!(format!("Studio discovery failed: {error}")).to_be_empty()?;
                        return Ok(());
                    }
                };
                expect!(calls).to_equal(2)?;
                expect!(studio_ids).to_equal(vec!["studio-one".to_owned()])?;
            }

            "recognizes when Studio is waiting for sign-in" {
                expect!(log_indicates_sign_in_required(
                    "LoginDialog Error: Embedded Web Browser fail to load"
                ))
                .to_be_true()?;
            }
        }

        "Adding the launcher to an AI client's configuration" {
            setup {
                let unique_stamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
                    Ok(duration) => duration.as_nanos(),
                    Err(error) => {
                        expect!(format!("the test clock failed: {error}")).to_be_empty()?;
                        return Ok(());
                    }
                };
                let test_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target")
                    .join(format!(
                        "mcp-config-test-{}-{unique_stamp}",
                        std::process::id()
                    ));
                expect!(fs::create_dir_all(&test_root)).to_be_ok()?;
                let client_config = test_root.join("client.json");
                expect!(fs::write(
                    &client_config,
                    r#"{
  "mcpServers": {
    "Existing_Server": {
      "command": "existing-client",
      "args": ["--keep-me"]
    }
  },
  "metadata": {"keep": true}
}
"#,
                ))
                .to_be_ok()?;
            }

            "keeps the client's other servers and creates a backup" {
                let serialized = match setup_client_configuration(
                    PathBuf::from("/launcher/config.ini").as_path(),
                    &client_config,
                    false,
                ) {
                    Ok(serialized) => serialized,
                    Err(error) => {
                        expect!(format!("the launcher configuration failed: {error}"))
                            .to_be_empty()?;
                        return Ok(());
                    }
                };
                let parsed = match serde_json::from_str::<Value>(&serialized) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        expect!(format!("the merged configuration was not JSON: {error}"))
                            .to_be_empty()?;
                        return Ok(());
                    }
                };
                let servers = parsed.get("mcpServers").and_then(Value::as_object);
                expect!(servers.as_ref()).to_be_some()?;
                let Some(servers) = servers else {
                    return Ok(());
                };
                expect!(servers.contains_key("Existing_Server")).to_be_true()?;
                expect!(servers.contains_key("Roblox_Studio")).to_be_true()?;
                expect!(parsed["metadata"]["keep"].clone()).to_equal(Value::Bool(true))?;
                expect!(client_config.with_extension("json.bak").is_file()).to_be_true()?;
                expect!(fs::remove_dir_all(test_root)).to_be_ok()?;
            }
        }
    }
}
