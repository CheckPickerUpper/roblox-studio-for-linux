use crate::config::{default_config_path, load_config, save_config, LauncherConfig};
use crate::deployment::install_latest_studio;
use crate::desktop;
use crate::error::LauncherError;
use crate::runtime::{
    configure_webview2_runtime, configure_wine_prefix, discover_studio_executable,
    ensure_webview2_runtime, resolve_wine_binary, run_studio, run_studio_auth, run_wine,
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
  doctor       Check Wine, the prefix, and the Studio installation.
  configure    Save Wine and Studio paths.
  install      Install the current Studio deployment directly from Roblox.
  launch       Launch the newest installed Studio executable.
  register     Register the browser login callback with the desktop.

Configure options:
  --wine-binary PATH          Wine command or executable path.
  --wine-prefix PATH          Wine prefix directory.
  --studio-executable PATH    Fallback path to RobloxStudioBeta.exe.

Install options:
  --installer PATH             Run a locally downloaded bootstrapper through Wine.

Launch arguments:
  Arguments after launch are passed to RobloxStudioBeta.exe.
"#;

enum Command {
    Help,
    Register,
    Doctor,
    Configure {
        wine_binary: Option<String>,
        wine_prefix: Option<PathBuf>,
        studio_executable: Option<PathBuf>,
    },
    Install {
        installer: Option<PathBuf>,
    },
    Launch {
        studio_arguments: Vec<String>,
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
        Command::Register => {
            desktop::register_auth_handler()?;
            Ok(SUCCESS_EXIT_CODE)
        }
        Command::Doctor => {
            let launcher_config = load_config(&config_path)?;
            report_launcher_doctor(&launcher_config)
        }
        Command::Configure {
            wine_binary,
            wine_prefix,
            studio_executable,
        } => {
            let launcher_config = load_config(&config_path)?;
            configure_launcher(
                &launcher_config,
                wine_binary,
                wine_prefix,
                studio_executable,
            )
        }
        Command::Install { installer } => {
            let launcher_config = load_config(&config_path)?;
            install_studio(&launcher_config, installer)
        }
        Command::Launch { studio_arguments } => {
            let launcher_config = load_config(&config_path)?;
            launch_latest_studio(&launcher_config, &studio_arguments)
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
        "register" => match tokens.split_first() {
            None => Command::Register,
            Some(_) => {
                return Err(invalid_arguments(
                    "register does not accept arguments".to_owned(),
                    &tokens,
                ));
            }
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

fn parse_configure_arguments(tokens: &[String]) -> Result<Command, LauncherError> {
    let mut wine_binary = None;
    let mut wine_prefix = None;
    let mut studio_executable = None;
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

    match resolve_wine_binary(&launcher_config.wine_binary) {
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

    let discovered = discover_studio_executable(&launcher_config.wine_prefix)?;
    let selected = match discovered {
        Some(path) => Some(path),
        None => match &launcher_config.studio_executable {
            Some(path) => match path.is_file() {
                true => Some(path.clone()),
                false => None,
            },
            None => None,
        },
    };

    match selected {
        Some(path) => tracing::info!(
            path = %path.display(),
            "Selected Studio executable"
        ),
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

fn configure_launcher(
    launcher_config: &LauncherConfig,
    wine_binary: Option<String>,
    wine_prefix: Option<PathBuf>,
    studio_executable: Option<PathBuf>,
) -> Result<i32, LauncherError> {
    let selected_wine_binary = match wine_binary {
        Some(value) => value,
        None => launcher_config.wine_binary.clone(),
    };
    let selected_wine_prefix = match wine_prefix {
        Some(path) => path,
        None => launcher_config.wine_prefix.clone(),
    };
    let selected_studio_executable = match studio_executable {
        Some(path) => Some(path),
        None => launcher_config.studio_executable.clone(),
    };
    let updated_config = LauncherConfig {
        config_path: launcher_config.config_path.clone(),
        wine_binary: selected_wine_binary,
        wine_prefix: selected_wine_prefix,
        studio_executable: selected_studio_executable,
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

    let exit_code =
        ensure_webview2_runtime(&wine_path, &launcher_config.wine_prefix, &studio_executable)?;
    if exit_code != SUCCESS_EXIT_CODE {
        return Ok(exit_code);
    }

    configure_webview2_override(&wine_path, &launcher_config.wine_prefix)
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

    match discover_studio_executable(&launcher_config.wine_prefix)? {
        Some(path) => {
            tracing::info!(
                path = %path.display(),
                "Latest installed Studio"
            );
            let exit_code =
                ensure_webview2_runtime(&wine_path, &launcher_config.wine_prefix, &path)?;
            if exit_code != SUCCESS_EXIT_CODE {
                return Ok(exit_code);
            }
            configure_webview2_override(&wine_path, &launcher_config.wine_prefix)
        }
        None => {
            tracing::warn!("Installer finished without a discovered Studio executable");
            Ok(SUCCESS_EXIT_CODE)
        }
    }
}

fn launch_latest_studio(
    launcher_config: &LauncherConfig,
    studio_arguments: &[String],
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

    let studio_executable = match discover_studio_executable(&launcher_config.wine_prefix)? {
        Some(path) => path,
        None => match &launcher_config.studio_executable {
            Some(path) => match path.is_file() {
                true => path.clone(),
                false => {
                    tracing::error!(
                        path = %path.display(),
                        "Configured Studio fallback is missing"
                    );
                    return Ok(INVALID_ARGUMENT_EXIT_CODE);
                }
            },
            None => {
                tracing::error!("RobloxStudioBeta.exe was not found; run install first");
                return Ok(INVALID_ARGUMENT_EXIT_CODE);
            }
        },
    };

    let exit_code = configure_wine_prefix(&wine_path, &launcher_config.wine_prefix)?;
    if exit_code != SUCCESS_EXIT_CODE {
        tracing::error!(exit_code, "Wine prefix Windows version setup failed");
        return Ok(exit_code);
    }

    let exit_code =
        ensure_webview2_runtime(&wine_path, &launcher_config.wine_prefix, &studio_executable)?;
    if exit_code != SUCCESS_EXIT_CODE {
        return Ok(exit_code);
    }

    let exit_code = configure_webview2_override(&wine_path, &launcher_config.wine_prefix)?;
    if exit_code != SUCCESS_EXIT_CODE {
        return Ok(exit_code);
    }

    register_auth_handler_best_effort();

    tracing::info!(
        path = %studio_executable.display(),
        "Launching latest Studio"
    );
    if studio_arguments
        .first()
        .is_some_and(|argument| argument.starts_with("roblox-studio-auth:"))
    {
        tracing::info!("Launching Studio authentication callback");
        return run_studio_auth(
            &wine_path,
            &launcher_config.wine_prefix,
            &studio_executable,
            studio_arguments,
        );
    }

    run_studio(
        &wine_path,
        &launcher_config.wine_prefix,
        &studio_executable,
        studio_arguments,
    )
}

fn configure_webview2_override(
    wine_path: &std::path::Path,
    wine_prefix: &std::path::Path,
) -> Result<i32, LauncherError> {
    let exit_code = configure_webview2_runtime(wine_path, wine_prefix)?;
    if exit_code != SUCCESS_EXIT_CODE {
        tracing::error!(exit_code, "WebView2 Wine override setup failed");
    }
    Ok(exit_code)
}

fn register_auth_handler_best_effort() {
    if let Err(error) = desktop::register_auth_handler() {
        tracing::warn!(
            error = %error,
            "Could not register the browser login handler"
        );
    }
}
