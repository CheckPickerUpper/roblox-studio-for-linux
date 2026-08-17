use crate::config::{default_config_path, load_config, save_config, LauncherConfig};
use crate::error::LauncherError;
use crate::runtime::{discover_studio_executable, resolve_wine_binary, run_wine};
use std::env;
use std::path::PathBuf;

const USAGE: &str = r#"Roblox Studio Linux Launcher

Usage:
  roblox-studio-linux-launcher [--config PATH] <command>

Commands:
  doctor       Check Wine, the prefix, and the Studio installation.
  configure    Save Wine and Studio paths.
  install      Run the official Windows Studio installer through Wine.
  launch       Launch the installed Studio executable.

Configure options:
  --wine-binary PATH          Wine command or executable path.
  --wine-prefix PATH          Wine prefix directory.
  --studio-executable PATH    Path to RobloxStudioBeta.exe.

Install options:
  --installer PATH             Path to the downloaded Studio installer.
"#;

enum Command {
    Help,
    Doctor,
    Configure {
        wine_binary: Option<String>,
        wine_prefix: Option<PathBuf>,
        studio_executable: Option<PathBuf>,
    },
    Install {
        installer: PathBuf,
    },
    Launch,
}

struct Arguments {
    config_path: PathBuf,
    command: Command,
}

pub fn run() -> Result<i32, LauncherError> {
    let arguments = parse(env::args().skip(1))?;
    if matches!(&arguments.command, Command::Help) {
        print!("{USAGE}");
        return Ok(0);
    }

    let config = load_config(&arguments.config_path)?;
    match arguments.command {
        Command::Help => Ok(0),
        Command::Doctor => doctor(&config),
        Command::Configure {
            wine_binary,
            wine_prefix,
            studio_executable,
        } => configure(&config, wine_binary, wine_prefix, studio_executable),
        Command::Install { installer } => install(&config, installer),
        Command::Launch => launch(&config),
    }
}

fn parse<I>(raw_arguments: I) -> Result<Arguments, LauncherError>
where
    I: IntoIterator<Item = String>,
{
    let mut tokens = raw_arguments.into_iter().collect::<Vec<_>>();
    if tokens.is_empty()
        || tokens
            .iter()
            .any(|token| token == "--help" || token == "-h")
    {
        return Ok(Arguments {
            config_path: default_config_path(),
            command: Command::Help,
        });
    }

    let config_path = if tokens.first().map(String::as_str) == Some("--config") {
        tokens.remove(0);
        PathBuf::from(required_value(&mut tokens, "--config")?)
    } else {
        default_config_path()
    };
    let command_name = tokens.remove(0);
    let command = match command_name.as_str() {
        "doctor" if tokens.is_empty() => Command::Doctor,
        "configure" => parse_configure(&tokens)?,
        "install" => parse_install(&tokens)?,
        "launch" if tokens.is_empty() => Command::Launch,
        "doctor" | "launch" => {
            return Err(LauncherError::InvalidArguments {
                message: format!("unexpected arguments for {command_name}"),
            });
        }
        _ => {
            return Err(LauncherError::InvalidArguments {
                message: format!("unknown command: {command_name}\n\n{USAGE}"),
            });
        }
    };
    Ok(Arguments {
        config_path,
        command,
    })
}

fn parse_configure(tokens: &[String]) -> Result<Command, LauncherError> {
    let mut wine_binary = None;
    let mut wine_prefix = None;
    let mut studio_executable = None;
    let mut index = 0;
    while index < tokens.len() {
        let option = &tokens[index];
        index += 1;
        match option.as_str() {
            "--wine-binary" => {
                wine_binary = Some(required_indexed_value(tokens, &mut index, option)?)
            }
            "--wine-prefix" => {
                wine_prefix = Some(PathBuf::from(required_indexed_value(
                    tokens, &mut index, option,
                )?));
            }
            "--studio-executable" => {
                studio_executable = Some(PathBuf::from(required_indexed_value(
                    tokens, &mut index, option,
                )?));
            }
            _ => {
                return Err(LauncherError::InvalidArguments {
                    message: format!("unknown configure option: {option}"),
                });
            }
        }
    }
    Ok(Command::Configure {
        wine_binary,
        wine_prefix,
        studio_executable,
    })
}

fn parse_install(tokens: &[String]) -> Result<Command, LauncherError> {
    if tokens.len() != 2 || tokens[0] != "--installer" {
        return Err(LauncherError::InvalidArguments {
            message: "install requires exactly --installer PATH".to_owned(),
        });
    }
    Ok(Command::Install {
        installer: PathBuf::from(&tokens[1]),
    })
}

fn required_value(tokens: &mut Vec<String>, option: &str) -> Result<String, LauncherError> {
    if tokens.is_empty() {
        return Err(LauncherError::InvalidArguments {
            message: format!("{option} requires a value"),
        });
    }
    Ok(tokens.remove(0))
}

fn required_indexed_value(
    tokens: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, LauncherError> {
    let Some(value) = tokens.get(*index) else {
        return Err(LauncherError::InvalidArguments {
            message: format!("{option} requires a value"),
        });
    };
    *index += 1;
    Ok(value.clone())
}

fn doctor(config: &LauncherConfig) -> Result<i32, LauncherError> {
    let mut issues = Vec::new();
    let wine_path = resolve_wine_binary(&config.wine_binary);
    println!("Config: {}", config.config_path.display());
    match &wine_path {
        Some(path) => println!("Wine: {}", path.display()),
        None => {
            println!("Wine: missing ({})", config.wine_binary);
            issues.push("Wine is not available on PATH.");
        }
    }

    println!("Wine prefix: {}", config.wine_prefix.display());
    if !config.wine_prefix.exists() {
        println!("Wine prefix: not created yet");
    }
    if let Some(path) = &config.studio_executable {
        println!("Configured Studio: {}", path.display());
        if !path.is_file() {
            issues.push("The configured Studio executable does not exist.");
        }
    }

    let discovered = discover_studio_executable(&config.wine_prefix)?;
    match discovered {
        Some(path) => println!("Discovered Studio: {}", path.display()),
        None => {
            println!("Discovered Studio: not found");
            issues.push("RobloxStudioBeta.exe was not found in the Wine prefix.");
        }
    }

    if issues.is_empty() {
        println!("Launcher environment looks ready.");
        return Ok(0);
    }
    for issue in issues {
        eprintln!("Issue: {issue}");
    }
    Ok(1)
}

fn configure(
    config: &LauncherConfig,
    wine_binary: Option<String>,
    wine_prefix: Option<PathBuf>,
    studio_executable: Option<PathBuf>,
) -> Result<i32, LauncherError> {
    let updated = LauncherConfig {
        config_path: config.config_path.clone(),
        wine_binary: wine_binary.unwrap_or_else(|| config.wine_binary.clone()),
        wine_prefix: wine_prefix.unwrap_or_else(|| config.wine_prefix.clone()),
        studio_executable: studio_executable.or_else(|| config.studio_executable.clone()),
    };
    save_config(&updated)?;
    println!("Saved configuration to {}", updated.config_path.display());
    Ok(0)
}

fn install(config: &LauncherConfig, installer: PathBuf) -> Result<i32, LauncherError> {
    if !installer.is_file() {
        eprintln!("Installer file was not found: {}", installer.display());
        return Ok(2);
    }
    let Some(wine_path) = resolve_wine_binary(&config.wine_binary) else {
        eprintln!("Wine command was not found: {}", config.wine_binary);
        return Ok(2);
    };

    println!("Running installer with Wine: {}", installer.display());
    let arguments = vec![installer.display().to_string()];
    let exit_code = run_wine(&wine_path, &config.wine_prefix, &arguments)?;
    if exit_code != 0 {
        eprintln!("Installer exited with status {exit_code}.");
        return Ok(exit_code);
    }
    let discovered = discover_studio_executable(&config.wine_prefix)?;
    match discovered {
        Some(path) => {
            let updated = LauncherConfig {
                studio_executable: Some(path.clone()),
                ..config.clone()
            };
            save_config(&updated)?;
            println!("Saved Studio executable: {}", path.display());
        }
        None => println!("Installer finished, but Studio was not found automatically."),
    }
    Ok(0)
}

fn launch(config: &LauncherConfig) -> Result<i32, LauncherError> {
    let Some(wine_path) = resolve_wine_binary(&config.wine_binary) else {
        eprintln!("Wine command was not found: {}", config.wine_binary);
        return Ok(2);
    };
    let studio_executable = match &config.studio_executable {
        Some(path) if path.is_file() => path.clone(),
        _ => match discover_studio_executable(&config.wine_prefix)? {
            Some(path) => path,
            None => {
                eprintln!("RobloxStudioBeta.exe was not found. Run install first.");
                return Ok(2);
            }
        },
    };

    println!("Launching Studio: {}", studio_executable.display());
    let arguments = vec![studio_executable.display().to_string()];
    run_wine(&wine_path, &config.wine_prefix, &arguments)
}
