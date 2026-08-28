use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum McpDoctorFinding {
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
    pub(crate) const fn exit_code(&self) -> i32 {
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
    use super::McpDoctorFinding;
    use behave::prelude::*;

    behave! {
        "Reporting an AI connection finding to another process" {
            "Studio is not running" {
                "uses a stable machine-readable status" {
                    let serialized = serde_json::to_string(&McpDoctorFinding::StudioNotRunning)?;
                    expect!(serialized)
                        .to_equal("{\"status\":\"studio_not_running\"}".to_owned())?;
                }
            }
        }
    }
}
