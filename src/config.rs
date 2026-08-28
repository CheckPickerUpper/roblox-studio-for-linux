use crate::error::LauncherError;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const APP_DIRECTORY_NAME: &str = "roblox-studio-linux-launcher";
const CONFIG_FILENAME: &str = "config.ini";
const DEFAULT_WINE_BINARY: &str = "wine";

/// Chooses where Studio presents the Roblox account sign-in page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum StudioLoginMode {
    /// Render the account page inside Studio's managed WebView2 runtime.
    #[default]
    EmbeddedWebView,
    /// Open Studio's one-time account page in the Linux browser.
    ExternalBrowser,
}

impl StudioLoginMode {
    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::EmbeddedWebView => "embedded",
            Self::ExternalBrowser => "browser",
        }
    }

    pub(crate) const fn configure_flag(self) -> &'static str {
        match self {
            Self::EmbeddedWebView => "--embedded-webview",
            Self::ExternalBrowser => "--browser-login",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LauncherConfig {
    pub config_path: PathBuf,
    pub wine_binary: String,
    pub wine_prefix: PathBuf,
    pub studio_executable: Option<PathBuf>,
    pub(crate) login_mode: StudioLoginMode,
}

pub fn default_config_path() -> PathBuf {
    data_directory().join(CONFIG_FILENAME)
}

pub fn load_config(config_path: &Path) -> Result<LauncherConfig, LauncherError> {
    let contents = match fs::read_to_string(config_path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(LauncherError::ReadConfig {
                path: config_path.to_path_buf(),
                source,
            });
        }
    };

    let mut section = String::new();
    let mut wine_binary = None;
    let mut wine_prefix = None;
    let mut studio_executable = None;
    let mut login_mode = None;

    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_owned();
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(LauncherError::InvalidConfig {
                path: config_path.to_path_buf(),
                line: line_number,
                message: "expected key = value".to_owned(),
            });
        };
        let key = raw_key.trim();
        let value = raw_value.trim();
        match (section.as_str(), key) {
            ("wine", "binary") => wine_binary = Some(value.to_owned()),
            ("wine", "prefix") => wine_prefix = Some(PathBuf::from(value)),
            ("studio", "executable") if !value.is_empty() => {
                studio_executable = Some(PathBuf::from(value));
            }
            ("studio", "executable") => studio_executable = None,
            ("studio", "login_mode") => record_login_mode(
                &mut login_mode,
                parse_login_mode(value, config_path, line_number)?,
                config_path,
                line_number,
            )?,
            ("studio", "embedded_webview") => record_login_mode(
                &mut login_mode,
                parse_legacy_login_mode(value, config_path, line_number)?,
                config_path,
                line_number,
            )?,
            _ => {}
        }
    }

    Ok(LauncherConfig {
        config_path: config_path.to_path_buf(),
        wine_binary: wine_binary
            .filter(|value: &String| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_WINE_BINARY.to_owned()),
        wine_prefix: wine_prefix.unwrap_or_else(|| {
            config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("wine")
        }),
        studio_executable,
        login_mode: login_mode.unwrap_or_default(),
    })
}

pub fn save_config(config: &LauncherConfig) -> Result<(), LauncherError> {
    let parent = config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| LauncherError::WriteConfig {
        path: config.config_path.clone(),
        source,
    })?;

    let executable = config
        .studio_executable
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let contents = format!(
        "[wine]\nbinary = {}\nprefix = {}\n\n[studio]\nexecutable = {}\nlogin_mode = {}\n",
        config.wine_binary,
        config.wine_prefix.display(),
        executable,
        config.login_mode.config_value(),
    );
    fs::write(&config.config_path, contents).map_err(|source| LauncherError::WriteConfig {
        path: config.config_path.clone(),
        source,
    })
}

fn parse_login_mode(
    value: &str,
    config_path: &Path,
    line_number: usize,
) -> Result<StudioLoginMode, LauncherError> {
    match value.to_ascii_lowercase().as_str() {
        "embedded" => Ok(StudioLoginMode::EmbeddedWebView),
        "browser" => Ok(StudioLoginMode::ExternalBrowser),
        _ => Err(LauncherError::InvalidConfig {
            path: config_path.to_path_buf(),
            line: line_number,
            message: "login_mode must be embedded or browser".to_owned(),
        }),
    }
}

fn parse_legacy_login_mode(
    value: &str,
    config_path: &Path,
    line_number: usize,
) -> Result<StudioLoginMode, LauncherError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => Ok(StudioLoginMode::EmbeddedWebView),
        "false" | "no" | "off" => Ok(StudioLoginMode::ExternalBrowser),
        _ => Err(LauncherError::InvalidConfig {
            path: config_path.to_path_buf(),
            line: line_number,
            message: "embedded_webview must be true or false".to_owned(),
        }),
    }
}

fn record_login_mode(
    selected_mode: &mut Option<StudioLoginMode>,
    mode: StudioLoginMode,
    config_path: &Path,
    line_number: usize,
) -> Result<(), LauncherError> {
    if selected_mode.is_some() {
        return Err(LauncherError::InvalidConfig {
            path: config_path.to_path_buf(),
            line: line_number,
            message: "Studio login mode is configured more than once".to_owned(),
        });
    }
    *selected_mode = Some(mode);
    Ok(())
}

fn data_directory() -> PathBuf {
    let base_directory = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base_directory.join(APP_DIRECTORY_NAME)
}

#[cfg(test)]
mod tests {
    use super::{
        load_config, parse_legacy_login_mode, parse_login_mode, save_config, LauncherConfig,
        StudioLoginMode,
    };
    use behave::prelude::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    behave! {
        "Choosing the Studio login method" {
            "a new launcher configuration" {
                "uses Studio's managed embedded login page by default" {
                    let config = load_config(Path::new(
                        "/tmp/roblox-studio-linux-launcher-config-that-does-not-exist",
                    ))?;
                    expect!(config.login_mode).to_equal(StudioLoginMode::EmbeddedWebView)?;
                }
            }

            "a saved launcher configuration" {
                setup {
                    let config_path = std::env::temp_dir().join(format!(
                        "roblox-studio-login-mode-{}.ini",
                        std::process::id(),
                    ));
                    let config = LauncherConfig {
                        config_path: config_path.clone(),
                        wine_binary: "wine".to_owned(),
                        wine_prefix: PathBuf::from("/tmp/roblox-studio-prefix"),
                        studio_executable: None,
                        login_mode: StudioLoginMode::EmbeddedWebView,
                    };
                    save_config(&config)?;
                    let saved_config = fs::read_to_string(&config_path)?;
                    let _ = fs::remove_file(&config_path);
                }

                "writes one named login mode instead of a second boolean authority" {
                    expect!(saved_config).to_equal(
                        "[wine]\nbinary = wine\nprefix = /tmp/roblox-studio-prefix\n\n[studio]\nexecutable = \nlogin_mode = embedded\n".to_owned(),
                    )?;
                }
            }

            "the user enables the embedded login page" {
                "accepts the named embedded setting" {
                    expect!(parse_login_mode(
                        "embedded",
                        Path::new("/tmp/launcher-config"),
                        4,
                    )?)
                    .to_equal(StudioLoginMode::EmbeddedWebView)?;
                }
            }

            "an older browser-login configuration" {
                "migrates to the named browser mode" {
                    expect!(parse_legacy_login_mode(
                        "false",
                        Path::new("/tmp/launcher-config"),
                        4,
                    )?)
                    .to_equal(StudioLoginMode::ExternalBrowser)?;
                }
            }

            "the user enters an invalid login setting" {
                "reports which setting is wrong" {
                    let result = parse_login_mode(
                        "sometimes",
                        Path::new("/tmp/launcher-config"),
                        7,
                    );
                    expect!(result.is_err()).to_equal(true)?;
                }
            }
        }
    }
}
