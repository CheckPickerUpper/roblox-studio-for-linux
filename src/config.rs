use crate::error::LauncherError;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const APP_DIRECTORY_NAME: &str = "roblox-studio-linux-launcher";
const CONFIG_FILENAME: &str = "config.ini";
const DEFAULT_WINE_BINARY: &str = "wine";

#[derive(Debug, Clone)]
pub struct LauncherConfig {
    pub config_path: PathBuf,
    pub wine_binary: String,
    pub wine_prefix: PathBuf,
    pub studio_executable: Option<PathBuf>,
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
        "[wine]\nbinary = {}\nprefix = {}\n\n[studio]\nexecutable = {}\n",
        config.wine_binary,
        config.wine_prefix.display(),
        executable
    );
    fs::write(&config.config_path, contents).map_err(|source| LauncherError::WriteConfig {
        path: config.config_path.clone(),
        source,
    })
}

fn data_directory() -> PathBuf {
    let base_directory = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base_directory.join(APP_DIRECTORY_NAME)
}
