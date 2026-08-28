use crate::config::{
    default_config_path, load_config, save_config, LauncherConfig, StudioLoginMode,
};
use crate::deployment::install_latest_studio;
use crate::desktop;
use crate::error::LauncherError;
use crate::mcp::{
    doctor_mcp, generate_client_configuration, route_auth_callback_if_needed, serve_mcp,
    setup_client_configuration,
};
use crate::runtime::{
    chromium_timestamp_now, configure_wine_prefix, latest_studio_auth_visit_time,
    prepare_studio_runtime, resolve_wine_binary, run_studio, run_studio_auth, run_wine,
    select_studio_installation, watch_for_studio_browser_login, StudioRuntimePlan,
};
use std::env;
use std::path::PathBuf;

const SUCCESS_EXIT_CODE: i32 = 0;
const CHECK_FAILED_EXIT_CODE: i32 = 1;
const INVALID_ARGUMENT_EXIT_CODE: i32 = 2;
const FIRST_ARGUMENT_INDEX: usize = 0;
const INDEX_STEP: usize = 1;

const USAGE: &str = r#"Roblox Studio Linux Launcher

Usage:
  roblox-studio-linux-launcher [--config PATH] <command>

Commands:
  gui          Open the graphical launcher.
  browser-login Open Studio and automatically open its sign-in page in the Linux browser.
  doctor       Check Wine, the prefix, and the Studio installation.
  configure    Save Wine and Studio paths.
  install      Install the current Studio deployment directly from Roblox.
  launch       Launch the newest installed Studio executable.
  register     Register the browser login callback with the desktop.
  mcp          Connect an AI client to the matching Studio MCP process.

Configure options:
  --wine-binary PATH          Wine command or executable path.
  --wine-prefix PATH          Wine prefix directory.
  --studio-executable PATH    Fallback path to RobloxStudioBeta.exe.
  --clear-studio-executable    Remove the configured Studio fallback.
  --browser-login              Use the Linux browser for Studio sign-in.
  --embedded-webview           Try Studio's embedded WebView2 login page.

Install options:
  --installer PATH             Run a locally downloaded bootstrapper through Wine.

Launch arguments:
  Arguments after launch are passed to RobloxStudioBeta.exe.

MCP commands:
  mcp serve                         Serve StudioMCP.exe over inherited stdio.
  mcp doctor                        Verify StudioMCP and a live Studio session.
  mcp setup --client-config PATH    Safely merge Roblox_Studio into client JSON.
  mcp setup --print                 Print a ready-to-copy client JSON configuration.
"#;

enum Command {
    Help,
    Gui,
    Register,
    BrowserLogin {
        studio_arguments: Vec<String>,
    },
    Doctor,
    Configure {
        wine_binary: Option<String>,
        wine_prefix: Option<PathBuf>,
        studio_executable: Option<PathBuf>,
        clear_studio_executable: bool,
        login_mode: Option<StudioLoginMode>,
    },
    Install {
        installer: Option<PathBuf>,
    },
    Launch {
        studio_arguments: Vec<String>,
    },
    Mcp {
        action: McpAction,
    },
}

enum McpAction {
    Serve,
    Doctor,
    Setup {
        client_config: Option<PathBuf>,
        print_only: bool,
    },
}

struct Arguments {
    config_path: PathBuf,
    command: Command,
}

/// Runs the requested launcher command and returns its process exit code.
pub fn run_launcher() -> Result<i32, LauncherError> {
    let Arguments {
        config_path,
        command,
    } = parse_launcher_arguments(env::args().skip(1))?;

    match command {
        Command::Help => {
            tracing::info!("{USAGE}");
            Ok(SUCCESS_EXIT_CODE)
        }
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
        Command::Configure {
            wine_binary,
            wine_prefix,
            studio_executable,
            clear_studio_executable,
            login_mode,
        } => {
            let launcher_config = load_config(&config_path)?;
            configure_launcher(
                &launcher_config,
                wine_binary,
                wine_prefix,
                studio_executable,
                clear_studio_executable,
                login_mode,
            )
        }
        Command::Install { installer } => {
            let launcher_config = load_config(&config_path)?;
            install_studio(&launcher_config, installer)
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

fn parse_launcher_arguments<I>(raw_arguments: I) -> Result<Arguments, LauncherError>
where
    I: IntoIterator<Item = String>,
{
    let mut tokens = raw_arguments.into_iter().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Ok(Arguments {
            config_path: default_config_path(),
            command: Command::Help,
        });
    }

    let config_path = match tokens.first().map(String::as_str) {
        Some("--config") => {
            tokens.remove(FIRST_ARGUMENT_INDEX);
            expand_user_path(PathBuf::from(take_required_value(&mut tokens, "--config")?))
        }
        Some(_) | None => default_config_path(),
    };

    let command_name = match tokens.first().cloned() {
        Some(command_name) => {
            tokens.remove(FIRST_ARGUMENT_INDEX);
            command_name
        }
        None => {
            return Err(invalid_arguments(
                "a command is required".to_owned(),
                &tokens,
            ));
        }
    };

    let command = match command_name.as_str() {
        "--help" | "-h" => Command::Help,
        "gui" => match tokens.split_first() {
            None => Command::Gui,
            Some(_) => {
                return Err(invalid_arguments(
                    "gui does not accept arguments".to_owned(),
                    &tokens,
                ));
            }
        },
        "register" => match tokens.split_first() {
            None => Command::Register,
            Some(_) => {
                return Err(invalid_arguments(
                    "register does not accept arguments".to_owned(),
                    &tokens,
                ));
            }
        },
        "browser-login" => Command::BrowserLogin {
            studio_arguments: tokens,
        },
        "doctor" => match tokens.split_first() {
            None => Command::Doctor,
            Some(_) => {
                return Err(invalid_arguments(
                    "doctor does not accept arguments".to_owned(),
                    &tokens,
                ));
            }
        },
        "configure" => parse_configure_arguments(&tokens)?,
        "install" => parse_install_arguments(&tokens)?,
        "launch" => Command::Launch {
            studio_arguments: tokens,
        },
        "mcp" => parse_mcp_arguments(&tokens)?,
        _ => {
            return Err(invalid_arguments(
                format!("unknown command: {command_name}\n\n{USAGE}"),
                &tokens,
            ));
        }
    };

    Ok(Arguments {
        config_path,
        command,
    })
}

fn parse_mcp_arguments(tokens: &[String]) -> Result<Command, LauncherError> {
    let Some(action_name) = tokens.first() else {
        return Err(invalid_arguments(
            "mcp requires serve, doctor, or setup".to_owned(),
            tokens,
        ));
    };
    match action_name.as_str() {
        "serve" => match tokens.len() {
            1 => Ok(Command::Mcp {
                action: McpAction::Serve,
            }),
            _ => Err(invalid_arguments(
                "mcp serve does not accept arguments".to_owned(),
                tokens,
            )),
        },
        "doctor" => match tokens.len() {
            1 => Ok(Command::Mcp {
                action: McpAction::Doctor,
            }),
            _ => Err(invalid_arguments(
                "mcp doctor does not accept arguments".to_owned(),
                tokens,
            )),
        },
        "setup" => parse_mcp_setup_arguments(tokens),
        _ => Err(invalid_arguments(
            format!("unknown mcp action: {action_name}"),
            tokens,
        )),
    }
}

fn parse_mcp_setup_arguments(tokens: &[String]) -> Result<Command, LauncherError> {
    let mut client_config = None;
    let mut print_only = false;
    let mut index = 1;
    while let Some(option) = tokens.get(index) {
        index += INDEX_STEP;
        match option.as_str() {
            "--client-config" => {
                client_config = Some(expand_user_path(PathBuf::from(take_indexed_value(
                    tokens, &mut index, option,
                )?)));
            }
            "--print" => print_only = true,
            _ => {
                return Err(invalid_arguments(
                    format!("unknown mcp setup option: {option}"),
                    tokens,
                ));
            }
        }
    }
    if !print_only && client_config.is_none() {
        return Err(invalid_arguments(
            "mcp setup requires --client-config PATH, or use --print".to_owned(),
            tokens,
        ));
    }
    Ok(Command::Mcp {
        action: McpAction::Setup {
            client_config,
            print_only,
        },
    })
}

fn parse_configure_arguments(tokens: &[String]) -> Result<Command, LauncherError> {
    let mut wine_binary = None;
    let mut wine_prefix = None;
    let mut studio_executable = None;
    let mut clear_studio_executable = false;
    let mut login_mode = None;
    let mut index = 0;

    while let Some(option) = tokens.get(index) {
        index += INDEX_STEP;
        match option.as_str() {
            "--wine-binary" => {
                wine_binary = Some(take_indexed_value(tokens, &mut index, option)?);
            }
            "--wine-prefix" => {
                wine_prefix = Some(expand_user_path(PathBuf::from(take_indexed_value(
                    tokens, &mut index, option,
                )?)));
            }
            "--studio-executable" => {
                studio_executable = Some(expand_user_path(PathBuf::from(take_indexed_value(
                    tokens, &mut index, option,
                )?)));
            }
            "--clear-studio-executable" => {
                clear_studio_executable = true;
            }
            "--browser-login" => login_mode = Some(StudioLoginMode::ExternalBrowser),
            "--embedded-webview" => login_mode = Some(StudioLoginMode::EmbeddedWebView),
            _ => {
                return Err(invalid_arguments(
                    format!("unknown configure option: {option}"),
                    tokens,
                ));
            }
        }
    }

    Ok(Command::Configure {
        wine_binary,
        wine_prefix,
        studio_executable,
        clear_studio_executable,
        login_mode,
    })
}

fn parse_install_arguments(tokens: &[String]) -> Result<Command, LauncherError> {
    match tokens {
        [] => Ok(Command::Install { installer: None }),
        [option, installer] => match option.as_str() {
            "--installer" => Ok(Command::Install {
                installer: Some(expand_user_path(PathBuf::from(installer))),
            }),
            _ => Err(invalid_arguments(
                "install accepts no arguments or exactly --installer PATH".to_owned(),
                tokens,
            )),
        },
        _ => Err(invalid_arguments(
            "install accepts no arguments or exactly --installer PATH".to_owned(),
            tokens,
        )),
    }
}

fn take_required_value(tokens: &mut Vec<String>, option: &str) -> Result<String, LauncherError> {
    match tokens.first().cloned() {
        Some(value) => {
            tokens.remove(FIRST_ARGUMENT_INDEX);
            Ok(value)
        }
        None => Err(invalid_arguments(
            format!("{option} requires a value"),
            tokens,
        )),
    }
}

fn take_indexed_value(
    tokens: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, LauncherError> {
    match tokens.get(*index).cloned() {
        Some(value) => {
            *index += INDEX_STEP;
            Ok(value)
        }
        None => Err(invalid_arguments(
            format!("{option} requires a value"),
            tokens,
        )),
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
        McpAction::Doctor => doctor_mcp(launcher_config),
        McpAction::Setup {
            client_config,
            print_only,
        } => {
            let serialized = match (client_config, print_only) {
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
        if let Some(exit_code) = route_auth_callback_if_needed(launcher_config, studio_arguments)? {
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
