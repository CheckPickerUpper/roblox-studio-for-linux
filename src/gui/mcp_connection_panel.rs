use super::command_completion::CommandCompletion;
use super::StatusTone;
use crate::mcp::McpDoctorFinding;

pub(super) struct McpConnectionPanel {
    state: ConnectionState,
    details: DetailsVisibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum McpCheckCompletion {
    Recognized,
    Unreadable,
}

enum ConnectionState {
    NotChecked,
    Checked {
        finding: McpDoctorFinding,
        diagnostics: String,
    },
    CheckFailed {
        message: String,
        diagnostics: String,
    },
}

#[derive(Clone, Copy)]
enum DetailsVisibility {
    Hidden,
    Visible,
}

impl McpConnectionPanel {
    pub(super) const fn new() -> Self {
        Self {
            state: ConnectionState::NotChecked,
            details: DetailsVisibility::Hidden,
        }
    }

    pub(super) fn complete_check(&mut self, completion: &CommandCompletion) -> McpCheckCompletion {
        let diagnostics = completion.diagnostics();
        match serde_json::from_str::<McpDoctorFinding>(completion.stdout().trim()) {
            Ok(finding) => {
                self.state = ConnectionState::Checked {
                    finding,
                    diagnostics,
                };
                McpCheckCompletion::Recognized
            }
            Err(error) => {
                self.state = ConnectionState::CheckFailed {
                    message: format!("The connection check returned an unreadable result: {error}"),
                    diagnostics,
                };
                self.details = DetailsVisibility::Visible;
                McpCheckCompletion::Unreadable
            }
        }
    }

    pub(super) fn status(&self) -> String {
        match &self.state {
            ConnectionState::NotChecked => "Not checked".to_owned(),
            ConnectionState::Checked { finding, .. } => match finding {
                McpDoctorFinding::WineUnavailable => "Required support is unavailable".to_owned(),
                McpDoctorFinding::StudioUnavailable => "Studio is not installed".to_owned(),
                McpDoctorFinding::McpUnavailable { .. } => {
                    "Studio connection needs repair".to_owned()
                }
                McpDoctorFinding::ProcessUnavailable { .. } => {
                    "Studio connection could not start".to_owned()
                }
                McpDoctorFinding::StudioNotRunning => {
                    "Studio is not running with an open place".to_owned()
                }
                McpDoctorFinding::StudioPlaceNotOpen => "Open a Studio place first".to_owned(),
                McpDoctorFinding::StudioNeedsSignIn => "Studio needs sign-in".to_owned(),
                McpDoctorFinding::StudioMcpNotEnabled => {
                    "Turn on Studio's AI connection".to_owned()
                }
                McpDoctorFinding::RestartStudio { .. } => {
                    "Restart Studio to finish its update".to_owned()
                }
                McpDoctorFinding::StudioSessionUnavailable => {
                    "Studio connection could not be verified".to_owned()
                }
                McpDoctorFinding::MultipleStudioSessions { .. } => {
                    "Close extra Studio windows, then try again".to_owned()
                }
                McpDoctorFinding::Connected => "Connected to Studio".to_owned(),
            },
            ConnectionState::CheckFailed { message, .. } => message.clone(),
        }
    }

    pub(super) const fn tone(&self) -> StatusTone {
        match &self.state {
            ConnectionState::NotChecked => StatusTone::Warning,
            ConnectionState::Checked { finding, .. } => match finding {
                McpDoctorFinding::Connected => StatusTone::Success,
                McpDoctorFinding::WineUnavailable
                | McpDoctorFinding::StudioUnavailable
                | McpDoctorFinding::McpUnavailable { .. }
                | McpDoctorFinding::ProcessUnavailable { .. } => StatusTone::Error,
                McpDoctorFinding::StudioNotRunning
                | McpDoctorFinding::StudioPlaceNotOpen
                | McpDoctorFinding::StudioNeedsSignIn
                | McpDoctorFinding::StudioMcpNotEnabled
                | McpDoctorFinding::RestartStudio { .. }
                | McpDoctorFinding::StudioSessionUnavailable
                | McpDoctorFinding::MultipleStudioSessions { .. } => StatusTone::Warning,
            },
            ConnectionState::CheckFailed { .. } => StatusTone::Error,
        }
    }

    pub(super) fn guidance(&self) -> Option<&'static str> {
        match &self.state {
            ConnectionState::Checked {
                finding: McpDoctorFinding::StudioMcpNotEnabled,
                ..
            } => Some("In Studio, open Assistant, open its menu, then turn on Studio access."),
            ConnectionState::Checked {
                finding: McpDoctorFinding::StudioNeedsSignIn,
                ..
            } => Some("Finish signing in to Studio, then check the connection again."),
            ConnectionState::Checked {
                finding: McpDoctorFinding::StudioPlaceNotOpen,
                ..
            } => Some("Open a place in Studio, then check the connection again."),
            ConnectionState::Checked {
                finding: McpDoctorFinding::StudioNotRunning,
                ..
            } => Some("Launch Studio and open a place, then check the connection again."),
            ConnectionState::NotChecked
            | ConnectionState::Checked { .. }
            | ConnectionState::CheckFailed { .. } => None,
        }
    }

    pub(super) fn diagnostics(&self) -> String {
        match &self.state {
            ConnectionState::NotChecked => {
                "Run Check connection while Studio has a place open.".to_owned()
            }
            ConnectionState::Checked { diagnostics, .. }
            | ConnectionState::CheckFailed { diagnostics, .. } => diagnostics.clone(),
        }
    }

    pub(super) const fn details_are_visible(&self) -> bool {
        matches!(self.details, DetailsVisibility::Visible)
    }

    pub(super) fn toggle_details(&mut self) {
        self.details = match self.details {
            DetailsVisibility::Hidden => DetailsVisibility::Visible,
            DetailsVisibility::Visible => DetailsVisibility::Hidden,
        };
    }
}
