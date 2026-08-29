use crate::config::{
    default_config_path, load_config, save_config, LauncherConfig, StudioLoginMode,
};
use crate::deployment::install_latest_studio;
use crate::desktop;
use crate::error::LauncherError;
use crate::mcp::{
    doctor_mcp, generate_client_configuration, serve_mcp, setup_client_configuration,
    McpDoctorOutput,
};
use crate::platform::ActiveStudioInvocation;
use crate::runtime::{
    chromium_timestamp_now, configure_wine_prefix, latest_studio_auth_visit_time,
    prepare_studio_runtime, resolve_wine_binary, run_studio, run_studio_auth, run_wine,
    select_studio_installation, watch_for_studio_browser_login, StudioRuntimePlan,
};
use clap::{ArgGroup, Args as ClapArgs, Parser, Subcommand};
use std::env;
use std::path::PathBuf;

const SUCCESS_EXIT_CODE: i32 = 0;
const CHECK_FAILED_EXIT_CODE: i32 = 1;
const INVALID_ARGUMENT_EXIT_CODE: i32 = 2;

#[derive(Parser)]
#[command(
    name = "roblox-studio-linux-launcher",
    version,
    about = "Launch and maintain Roblox Studio on Linux",
    disable_help_subcommand = true
)]
struct Arguments {
    #[arg(
        long,
        value_name = "PATH",
        help = "Use a specific launcher configuration file"
    )]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the graphical launcher.
    Gui,
    /// Register the browser login callback with the Linux desktop.
    Register,
    /// Open Studio and its sign-in page in the Linux browser.
    BrowserLogin {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        studio_arguments: Vec<String>,
    },
    /// Check Wine, the prefix, and the Studio installation.
    Doctor,
    /// Save Wine, Studio, and login settings.
    Configure(ConfigureArguments),
    /// Install the current Studio deployment from Roblox.
    Install {
        #[arg(long, value_name = "PATH")]
        installer: Option<PathBuf>,
    },
    /// Launch the newest installed Studio executable.
    Launch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        studio_arguments: Vec<String>,
    },
    /// Set up and check AI access to the open Studio place.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(ClapArgs)]
struct ConfigureArguments {
    /// Wine command or executable path.
    #[arg(long, value_name = "PATH")]
    wine_binary: Option<String>,
    /// Wine prefix directory.
    #[arg(long, value_name = "PATH")]
    wine_prefix: Option<PathBuf>,
    /// Fallback path to RobloxStudioBeta.exe.
    #[arg(long, value_name = "PATH", conflicts_with = "clear_studio_executable")]
    studio_executable: Option<PathBuf>,
    /// Remove the configured Studio fallback.
    #[arg(long)]
    clear_studio_executable: bool,
    /// Use the Linux browser for Studio sign-in.
    #[arg(long, conflicts_with = "embedded_webview")]
    browser_login: bool,
    /// Try Studio's embedded WebView2 login page.
    #[arg(long)]
    embedded_webview: bool,
}

impl ConfigureArguments {
    fn login_mode(&self) -> Option<StudioLoginMode> {
        match (self.browser_login, self.embedded_webview) {
            (true, false) => Some(StudioLoginMode::ExternalBrowser),
            (false, true) => Some(StudioLoginMode::EmbeddedWebView),
            (false, false) => None,
            (true, true) => None,
        }
    }
}

#[derive(Subcommand)]
enum McpAction {
    /// Serve the matching StudioMCP executable over inherited stdio.
    Serve,
    /// Verify StudioMCP and a live Studio session.
    Doctor(McpDoctorArguments),
    /// Add the launcher to an AI client's MCP configuration.
    Setup(McpSetupArguments),
}

#[derive(ClapArgs)]
struct McpDoctorArguments {
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(ClapArgs)]
#[command(group(
    ArgGroup::new("destination")
        .required(true)
        .multiple(false)
        .args(["client_config", "print"])
))]
struct McpSetupArguments {
    /// Safely merge Roblox_Studio into this client JSON file.
    #[arg(long, value_name = "PATH")]
    client_config: Option<PathBuf>,
    /// Print a ready-to-copy client JSON configuration.
    #[arg(long)]
    print: bool,
}

enum ParsedArguments {
    Run(Arguments),
    Display(String),
}

/// Runs the requested launcher command and returns its process exit code.
pub fn run_launcher() -> Result<i32, LauncherError> {
    let parsed_arguments = parse_launcher_arguments(env::args().skip(1))?;
    let Arguments { config, command } = match parsed_arguments {
        ParsedArguments::Run(arguments) => arguments,
        ParsedArguments::Display(help) => {
            // @why cli-output: clap's generated help and version text are the CLI's public output.
            print!("{help}");
            return Ok(SUCCESS_EXIT_CODE);
        }
    };
    let config_path = config
        .map(expand_user_path)
        .unwrap_or_else(default_config_path);
    let command = command_or_default(command);

    match command {
        Command::Gui => {
            crate::gui::run_gui(config_path)?;
            Ok(SUCCESS_EXIT_CODE)
        }
        Command::Register => {
            desktop::register_auth_handler()?;
            Ok(SUCCESS_EXIT_CODE)
        }
        Command::BrowserLogin { studio_arguments } => {
            let launcher_config = load_config(&config_path)?;
            launch_latest_studio(
                &launcher_config,
                StudioLoginMode::ExternalBrowser,
                &studio_arguments,
            )
        }
        Command::Doctor => {
            let launcher_config = load_config(&config_path)?;
            report_launcher_doctor(&launcher_config)
        }
        Command::Configure(configuration) => {
            let launcher_config = load_config(&config_path)?;
            let login_mode = configuration.login_mode();
            configure_launcher(
                &launcher_config,
                configuration.wine_binary,
                configuration.wine_prefix.map(expand_user_path),
                configuration.studio_executable.map(expand_user_path),
                configuration.clear_studio_executable,
                login_mode,
            )
        }
        Command::Install { installer } => {
            let launcher_config = load_config(&config_path)?;
            install_studio(&launcher_config, installer.map(expand_user_path))
        }
        Command::Launch { studio_arguments } => {
            let launcher_config = load_config(&config_path)?;
            launch_latest_studio(
                &launcher_config,
                launcher_config.login_mode,
                &studio_arguments,
            )
        }
        Command::Mcp { action } => {
            let launcher_config = load_config(&config_path)?;
            run_mcp_action(&launcher_config, action)
        }
    }
}

fn command_or_default(command: Option<Command>) -> Command {
    command.unwrap_or(Command::Gui)
}

fn parse_launcher_arguments<I>(raw_arguments: I) -> Result<ParsedArguments, LauncherError>
where
    I: IntoIterator<Item = String>,
{
    let provided_arguments = raw_arguments.into_iter().collect::<Vec<_>>();
    let parser_arguments = std::iter::once(env!("CARGO_PKG_NAME").to_owned())
        .chain(provided_arguments.iter().cloned());
    match Arguments::try_parse_from(parser_arguments) {
        Ok(arguments) => Ok(ParsedArguments::Run(arguments)),
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            Ok(ParsedArguments::Display(error.to_string()))
        }
        Err(error) => Err(invalid_arguments(error.to_string(), &provided_arguments)),
    }
}

fn invalid_arguments(message: String, provided_arguments: &[String]) -> LauncherError {
    LauncherError::InvalidArguments {
        message,
        provided_arguments: provided_arguments.to_vec(),
    }
}

fn expand_user_path(path: PathBuf) -> PathBuf {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return path;
    };
    let Some(path_string) = path.to_str() else {
        return path;
    };

    match path_string {
        "~" => home,
        _ => match path_string.strip_prefix("~/") {
            Some(relative) => home.join(relative),
            None => path,
        },
    }
}

fn report_launcher_doctor(launcher_config: &LauncherConfig) -> Result<i32, LauncherError> {
    let mut issues = Vec::new();
    tracing::info!(
        path = %launcher_config.config_path.display(),
        "Launcher configuration"
    );

    let resolved_wine_binary = resolve_wine_binary(&launcher_config.wine_binary);
    match &resolved_wine_binary {
        Some(path) => tracing::info!(path = %path.display(), "Wine executable"),
        None => {
            tracing::warn!(
                binary = %launcher_config.wine_binary,
                "Wine executable is unavailable"
            );
            issues.push("Wine is not available on PATH.".to_owned());
        }
    }

    tracing::info!(
        path = %launcher_config.wine_prefix.display(),
        "Wine prefix"
    );
    match launcher_config.wine_prefix.exists() {
        true => {}
        false => tracing::info!("Wine prefix has not been created yet"),
    }

    if let Some(path) = &launcher_config.studio_executable {
        tracing::info!(
            path = %path.display(),
            "Configured Studio fallback"
        );
        match path.is_file() {
            true => {}
            false => tracing::warn!("Configured Studio fallback is missing"),
        }
    }

    let selected_installation = match resolved_wine_binary {
        Some(wine_binary) => select_studio_installation(
            &wine_binary,
            &launcher_config.wine_prefix,
            launcher_config.studio_executable.as_deref(),
        )?,
        None => None,
    };
    match selected_installation {
        Some(installation) => {
            tracing::info!(
                path = %installation.studio_executable.display(),
                "Selected Studio executable"
            );
            if installation.mcp_executable.is_file() {
                tracing::info!(
                    path = %installation.mcp_executable.display(),
                    version = %installation.studio_version,
                    "Matching Studio MCP executable"
                );
            } else {
                tracing::warn!(
                    path = %installation.version_directory.display(),
                    "Matching StudioMCP.exe is missing"
                );
                issues.push(
                        "StudioMCP.exe is missing from the selected Studio version; run install to repair it."
                            .to_owned(),
                    );
            }
        }
        None => {
            tracing::warn!("Studio executable is unavailable");
            issues.push(
                "RobloxStudioBeta.exe was not found in the Wine prefix or configured path."
                    .to_owned(),
            );
        }
    }

    match issues.is_empty() {
        true => {
            tracing::info!("Launcher environment looks ready");
            Ok(SUCCESS_EXIT_CODE)
        }
        false => {
            for issue in issues {
                tracing::error!(issue = %issue, "Launcher issue");
            }
            Ok(CHECK_FAILED_EXIT_CODE)
        }
    }
}

fn run_mcp_action(
    launcher_config: &LauncherConfig,
    action: McpAction,
) -> Result<i32, LauncherError> {
    match action {
        McpAction::Serve => serve_mcp(launcher_config),
        McpAction::Doctor(arguments) => {
            let output = match arguments.json {
                true => McpDoctorOutput::Json,
                false => McpDoctorOutput::HumanReadable,
            };
            doctor_mcp(launcher_config, output)
        }
        McpAction::Setup(arguments) => {
            let client_config = arguments.client_config.map(expand_user_path);
            let serialized = match (client_config, arguments.print) {
                (Some(path), false) => {
                    setup_client_configuration(&launcher_config.config_path, &path, false)?
                }
                (None, true) | (Some(_), true) => {
                    generate_client_configuration(&launcher_config.config_path)?
                }
                (None, false) => {
                    return Err(invalid_arguments(
                        "mcp setup requires --client-config PATH, or use --print".to_owned(),
                        &[],
                    ));
                }
            };
            // @why cli-output: setup --print and successful setup are consumed as JSON by users and clients.
            println!("{serialized}");
            Ok(SUCCESS_EXIT_CODE)
        }
    }
}

fn configure_launcher(
    launcher_config: &LauncherConfig,
    wine_binary: Option<String>,
    wine_prefix: Option<PathBuf>,
    studio_executable: Option<PathBuf>,
    clear_studio_executable: bool,
    login_mode: Option<StudioLoginMode>,
) -> Result<i32, LauncherError> {
    let selected_wine_binary = match wine_binary {
        Some(value) => value,
        None => launcher_config.wine_binary.clone(),
    };
    let selected_wine_prefix = match wine_prefix {
        Some(path) => path,
        None => launcher_config.wine_prefix.clone(),
    };
    let selected_studio_executable = if clear_studio_executable {
        None
    } else {
        match studio_executable {
            Some(path) => Some(path),
            None => launcher_config.studio_executable.clone(),
        }
    };
    let selected_login_mode = login_mode.unwrap_or(launcher_config.login_mode);
    let updated_config = LauncherConfig {
        config_path: launcher_config.config_path.clone(),
        wine_binary: selected_wine_binary,
        wine_prefix: selected_wine_prefix,
        studio_executable: selected_studio_executable,
        login_mode: selected_login_mode,
    };

    save_config(&updated_config)?;
    tracing::info!(
        path = %updated_config.config_path.display(),
        "Saved launcher configuration"
    );
    Ok(SUCCESS_EXIT_CODE)
}

fn install_studio(
    launcher_config: &LauncherConfig,
    installer: Option<PathBuf>,
) -> Result<i32, LauncherError> {
    let exit_code = match installer {
        Some(path) => install_studio_with_bootstrapper(launcher_config, path)?,
        None => install_latest_studio_deployment(launcher_config)?,
    };
    if exit_code == SUCCESS_EXIT_CODE {
        register_auth_handler_best_effort();
    }
    Ok(exit_code)
}

fn install_latest_studio_deployment(
    launcher_config: &LauncherConfig,
) -> Result<i32, LauncherError> {
    let wine_path = match resolve_wine_binary(&launcher_config.wine_binary) {
        Some(path) => path,
        None => {
            tracing::error!(
                binary = %launcher_config.wine_binary,
                "Wine command is unavailable"
            );
            return Ok(INVALID_ARGUMENT_EXIT_CODE);
        }
    };
    tracing::debug!(path = %wine_path.display(), "Wine executable is available");

    let exit_code = configure_wine_prefix(&wine_path, &launcher_config.wine_prefix)?;
    if exit_code != SUCCESS_EXIT_CODE {
        tracing::error!(exit_code, "Wine prefix Windows version setup failed");
        return Ok(exit_code);
    }

    let studio_executable = install_latest_studio(&launcher_config.wine_prefix)?;
    tracing::info!(
        path = %studio_executable.display(),
        "Installed current Studio deployment"
    );
    let installation = select_studio_installation(&wine_path, &launcher_config.wine_prefix, None)?
        .ok_or_else(|| LauncherError::McpRuntimeUnavailable {
            message: "the deployment finished without a discoverable Studio installation"
                .to_owned(),
        })?;
    if !installation.mcp_executable.is_file() {
        return Err(LauncherError::MissingMcpExecutable {
            path: installation.mcp_executable,
        });
    }

    let plan = StudioRuntimePlan::new(launcher_config.login_mode);
    let exit_code = prepare_studio_runtime(
        plan,
        &wine_path,
        &launcher_config.wine_prefix,
        &studio_executable,
    )?;
    if exit_code == SUCCESS_EXIT_CODE {
        tracing::warn!(
            "Restart Roblox Studio after an install or update before testing its MCP connection"
        );
    }
    Ok(exit_code)
}

fn install_studio_with_bootstrapper(
    launcher_config: &LauncherConfig,
    installer: PathBuf,
) -> Result<i32, LauncherError> {
    match installer.is_file() {
        true => {}
        false => {
            tracing::error!(
                path = %installer.display(),
                "Studio installer is unavailable"
            );
            return Ok(INVALID_ARGUMENT_EXIT_CODE);
        }
    }

    let wine_path = match resolve_wine_binary(&launcher_config.wine_binary) {
        Some(path) => path,
        None => {
            tracing::error!(
                binary = %launcher_config.wine_binary,
                "Wine command is unavailable"
            );
            return Ok(INVALID_ARGUMENT_EXIT_CODE);
        }
    };

    let exit_code = configure_wine_prefix(&wine_path, &launcher_config.wine_prefix)?;
    if exit_code != SUCCESS_EXIT_CODE {
        tracing::error!(exit_code, "Wine prefix Windows version setup failed");
        return Ok(exit_code);
    }

    tracing::info!(
        path = %installer.display(),
        "Running Studio installer through Wine"
    );
    let installer_arguments = vec![installer.display().to_string()];
    let exit_code = run_wine(
        &wine_path,
        &launcher_config.wine_prefix,
        &installer_arguments,
    )?;
    if exit_code != SUCCESS_EXIT_CODE {
        tracing::error!(exit_code, "Studio installer exited unsuccessfully");
        return Ok(exit_code);
    }

    match select_studio_installation(
        &wine_path,
        &launcher_config.wine_prefix,
        launcher_config.studio_executable.as_deref(),
    )? {
        Some(installation) => {
            let path = installation.studio_executable;
            if !installation.mcp_executable.is_file() {
                return Err(LauncherError::MissingMcpExecutable {
                    path: installation.mcp_executable,
                });
            }
            tracing::info!(
                path = %path.display(),
                "Latest installed Studio"
            );
            let plan = StudioRuntimePlan::new(launcher_config.login_mode);
            let exit_code =
                prepare_studio_runtime(plan, &wine_path, &launcher_config.wine_prefix, &path)?;
            if exit_code == SUCCESS_EXIT_CODE {
                tracing::warn!(
                    "Restart Roblox Studio after an install or update before testing its MCP connection"
                );
            }
            Ok(exit_code)
        }
        None => {
            tracing::error!("Installer finished without a discoverable Studio installation");
            Ok(INVALID_ARGUMENT_EXIT_CODE)
        }
    }
}

fn launch_latest_studio(
    launcher_config: &LauncherConfig,
    login_mode: StudioLoginMode,
    studio_arguments: &[String],
) -> Result<i32, LauncherError> {
    let plan = StudioRuntimePlan::new(login_mode);
    let is_auth_callback = studio_arguments
        .first()
        .is_some_and(|argument| argument.starts_with("roblox-studio-auth:"));
    if is_auth_callback {
        let arguments = std::iter::once(std::ffi::OsString::from("launch"))
            .chain(studio_arguments.iter().map(std::ffi::OsString::from));
        let invocation = ActiveStudioInvocation::process(&launcher_config.config_path, arguments);
        if let Some(exit_code) = invocation.run_if_needed()? {
            return Ok(exit_code);
        }
    }

    let wine_path = match resolve_wine_binary(&launcher_config.wine_binary) {
        Some(path) => path,
        None => {
            tracing::error!(
                binary = %launcher_config.wine_binary,
                "Wine command is unavailable"
            );
            return Ok(INVALID_ARGUMENT_EXIT_CODE);
        }
    };

    let studio_installation = match select_studio_installation(
        &wine_path,
        &launcher_config.wine_prefix,
        launcher_config.studio_executable.as_deref(),
    )? {
        Some(installation) => installation,
        None => {
            tracing::error!("RobloxStudioBeta.exe was not found; run install first");
            return Ok(INVALID_ARGUMENT_EXIT_CODE);
        }
    };
    let studio_executable = studio_installation.studio_executable;

    let exit_code = configure_wine_prefix(&wine_path, &launcher_config.wine_prefix)?;
    if exit_code != SUCCESS_EXIT_CODE {
        tracing::error!(exit_code, "Wine prefix Windows version setup failed");
        return Ok(exit_code);
    }

    let browser_login_minimum_visit_time =
        if !is_auth_callback && plan.login_mode() == StudioLoginMode::ExternalBrowser {
            Some(
                latest_studio_auth_visit_time(&launcher_config.wine_prefix)
                    .ok()
                    .flatten()
                    .unwrap_or_else(chromium_timestamp_now),
            )
        } else {
            None
        };

    let exit_code = prepare_studio_runtime(
        plan,
        &wine_path,
        &launcher_config.wine_prefix,
        &studio_executable,
    )?;
    if exit_code != SUCCESS_EXIT_CODE {
        return Ok(exit_code);
    }

    if plan.login_mode() == StudioLoginMode::ExternalBrowser {
        desktop::register_auth_handler()?;
    } else {
        register_auth_handler_best_effort();
    }

    tracing::info!(
        path = %studio_executable.display(),
        "Launching latest Studio"
    );
    if is_auth_callback {
        tracing::info!("Launching Studio authentication callback");
        return run_studio_auth(
            plan,
            &wine_path,
            &launcher_config.wine_prefix,
            &studio_executable,
            studio_arguments,
        );
    }

    let exit_code = run_studio(
        plan,
        &wine_path,
        &launcher_config.wine_prefix,
        &studio_executable,
        studio_arguments,
    )?;
    if exit_code != SUCCESS_EXIT_CODE {
        return Ok(exit_code);
    }
    match browser_login_minimum_visit_time {
        Some(minimum_visit_time) => {
            watch_for_studio_browser_login(&launcher_config.wine_prefix, minimum_visit_time)
        }
        None => Ok(exit_code),
    }
}

fn register_auth_handler_best_effort() {
    if let Err(error) = desktop::register_auth_handler() {
        tracing::warn!(
            error = %error,
            "Could not register the browser login handler"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        command_or_default, parse_launcher_arguments, Command, McpAction, ParsedArguments,
    };
    use behave::prelude::*;

    behave! {
        "Understanding launcher commands" {
            // @behavior cli-default-gui
            // @boundary before=flatpak-command at=no-subcommand after=graphical-launcher
            // @red packaged-flatpak Running the app without arguments printed CLI help and exited.
            // @green default-gui The package command opens the graphical launcher by default.
            "The installed Flatpak is opened without command-line arguments" {
                "selects the graphical launcher" {
                    let parsed = parse_launcher_arguments(Vec::<String>::new());
                    let opens_gui = match parsed {
                        Ok(ParsedArguments::Run(arguments)) => {
                            matches!(command_or_default(arguments.command), Command::Gui)
                        }
                        Ok(ParsedArguments::Display(_)) | Err(_) => false,
                    };
                    expect!(opens_gui).to_be_true()?;
                }
            }

            // @behavior cli-forward-studio-arguments
            // @boundary before=launcher-options at=launch-command after=studio-options
            // @red hand-parser-removal A leading option-shaped Studio value could be consumed by the launcher.
            // @green clap-migration Every value after launch remains in Studio's argument list.
            "Studio options follow the launch command" {
                "forwards every value without interpreting it as a launcher option" {
                    let parsed = parse_launcher_arguments([
                        "launch".to_owned(),
                        "--config".to_owned(),
                        "studio-owned-value".to_owned(),
                    ]);
                    let studio_arguments = match parsed {
                        Ok(ParsedArguments::Run(arguments)) => match arguments.command {
                            Some(Command::Launch { studio_arguments }) => studio_arguments,
                            Some(_) | None => {
                                expect!("the launch command was not retained").to_be_empty()?;
                                return Ok(());
                            }
                        },
                        Ok(ParsedArguments::Display(_)) => {
                            expect!("help was displayed instead of parsing launch").to_be_empty()?;
                            return Ok(());
                        }
                        Err(error) => {
                            expect!(format!("launch parsing failed: {error}")).to_be_empty()?;
                            return Ok(());
                        }
                    };
                    expect!(studio_arguments).to_equal(vec![
                        "--config".to_owned(),
                        "studio-owned-value".to_owned(),
                    ])?;
                }
            }

            // @behavior cli-mcp-setup-destination
            // @boundary before=no-destination at=setup-parse after=one-destination
            // @red hand-parser-removal MCP setup could represent contradictory output choices.
            // @green clap-migration The parser accepts exactly one setup destination.
            "MCP setup receives one output destination" {
                "rejects a file destination combined with print-only output" {
                    let parsed = parse_launcher_arguments([
                        "mcp".to_owned(),
                        "setup".to_owned(),
                        "--client-config".to_owned(),
                        "client.json".to_owned(),
                        "--print".to_owned(),
                    ]);
                    expect!(parsed.is_err()).to_be_true()?;
                }

                "keeps a valid client configuration destination" {
                    let parsed = parse_launcher_arguments([
                        "mcp".to_owned(),
                        "setup".to_owned(),
                        "--client-config".to_owned(),
                        "client.json".to_owned(),
                    ]);
                    let is_setup = matches!(
                        parsed,
                        Ok(ParsedArguments::Run(arguments))
                            if matches!(
                                arguments.command,
                                Some(Command::Mcp {
                                    action: McpAction::Setup(_)
                                })
                            )
                    );
                    expect!(is_setup).to_be_true()?;
                }
            }

            // @behavior cli-generated-help
            // @boundary before=help-request at=argument-parse after=generated-help
            // @red hand-parser-removal Removing the static usage string could leave help as an error.
            // @green clap-migration Clap's generated help is returned as successful display output.
            "The user asks what commands are available" {
                "returns generated help instead of an invalid-command failure" {
                    let parsed = parse_launcher_arguments(["--help".to_owned()]);
                    let help = match parsed {
                        Ok(ParsedArguments::Display(help)) => help,
                        Ok(ParsedArguments::Run(_)) => {
                            expect!("the help request was treated as a command").to_be_empty()?;
                            return Ok(());
                        }
                        Err(error) => {
                            expect!(format!("help parsing failed: {error}")).to_be_empty()?;
                            return Ok(());
                        }
                    };
                    expect!(help.contains("Commands:")).to_be_true()?;
                    expect!(help.contains("mcp")).to_be_true()?;
                }
            }
        }
    }
}
