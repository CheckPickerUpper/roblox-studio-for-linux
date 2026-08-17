use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum LauncherError {
    InvalidArguments {
        message: String,
    },
    InvalidConfig {
        path: PathBuf,
        line: usize,
        message: String,
    },
    ReadConfig {
        path: PathBuf,
        source: io::Error,
    },
    WriteConfig {
        path: PathBuf,
        source: io::Error,
    },
    CreateWinePrefix {
        path: PathBuf,
        source: io::Error,
    },
    ReadDirectory {
        path: PathBuf,
        source: io::Error,
    },
    RunWine {
        program: String,
        source: io::Error,
    },
}

impl fmt::Display for LauncherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments { message } => write!(formatter, "{message}"),
            Self::InvalidConfig {
                path,
                line,
                message,
            } => write!(
                formatter,
                "invalid configuration at {} line {line}: {message}",
                path.display()
            ),
            Self::ReadConfig { path, source } => {
                write!(formatter, "could not read {}: {source}", path.display())
            }
            Self::WriteConfig { path, source } => {
                write!(formatter, "could not write {}: {source}", path.display())
            }
            Self::CreateWinePrefix { path, source } => {
                write!(
                    formatter,
                    "could not create Wine prefix {}: {source}",
                    path.display()
                )
            }
            Self::ReadDirectory { path, source } => {
                write!(formatter, "could not inspect {}: {source}", path.display())
            }
            Self::RunWine { program, source } => {
                write!(formatter, "could not start {program}: {source}")
            }
        }
    }
}

impl std::error::Error for LauncherError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadConfig { source, .. }
            | Self::WriteConfig { source, .. }
            | Self::CreateWinePrefix { source, .. }
            | Self::ReadDirectory { source, .. }
            | Self::RunWine { source, .. } => Some(source),
            Self::InvalidArguments { .. } | Self::InvalidConfig { .. } => None,
        }
    }
}
