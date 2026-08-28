use super::command_completion::CommandCompletion;
use super::launcher_action::LauncherAction;
use super::StatusTone;

pub(super) struct LastActionPanel {
    state: LastActionState,
    details: DetailsVisibility,
}

enum LastActionState {
    Ready,
    Completed {
        action: LauncherAction,
        completion: CommandCompletion,
    },
    DiagnosticCompleted {
        action: LauncherAction,
        diagnostics: String,
    },
    CouldNotFindLauncher {
        action: LauncherAction,
        message: String,
    },
    WorkerDisconnected {
        action: LauncherAction,
    },
    ConfigurationCopied {
        configuration: String,
    },
    ConfigurationCopyFailed {
        message: String,
    },
}

#[derive(Clone, Copy)]
enum DetailsVisibility {
    Hidden,
    Visible,
}

impl LastActionPanel {
    pub(super) const fn ready() -> Self {
        Self {
            state: LastActionState::Ready,
            details: DetailsVisibility::Hidden,
        }
    }

    pub(super) fn complete(&mut self, action: LauncherAction, completion: CommandCompletion) {
        self.details = if completion.succeeded() {
            DetailsVisibility::Hidden
        } else {
            DetailsVisibility::Visible
        };
        self.state = LastActionState::Completed { action, completion };
    }

    pub(super) fn could_not_find_launcher(&mut self, action: LauncherAction, message: String) {
        self.state = LastActionState::CouldNotFindLauncher { action, message };
        self.details = DetailsVisibility::Visible;
    }

    pub(super) fn diagnostic_completed(&mut self, action: LauncherAction, diagnostics: String) {
        self.state = LastActionState::DiagnosticCompleted {
            action,
            diagnostics,
        };
        self.details = DetailsVisibility::Hidden;
    }

    pub(super) fn worker_disconnected(&mut self, action: LauncherAction) {
        self.state = LastActionState::WorkerDisconnected { action };
        self.details = DetailsVisibility::Visible;
    }

    pub(super) fn configuration_copied(&mut self, configuration: String) {
        self.state = LastActionState::ConfigurationCopied { configuration };
        self.details = DetailsVisibility::Visible;
    }

    pub(super) fn configuration_copy_failed(&mut self, message: String) {
        self.state = LastActionState::ConfigurationCopyFailed { message };
        self.details = DetailsVisibility::Visible;
    }

    pub(super) const fn tone(&self) -> StatusTone {
        match &self.state {
            LastActionState::Ready => StatusTone::Neutral,
            LastActionState::Completed { completion, .. } if completion.succeeded() => {
                StatusTone::Success
            }
            LastActionState::ConfigurationCopied { .. } => StatusTone::Success,
            LastActionState::DiagnosticCompleted { .. } => StatusTone::Success,
            LastActionState::Completed { .. }
            | LastActionState::CouldNotFindLauncher { .. }
            | LastActionState::WorkerDisconnected { .. }
            | LastActionState::ConfigurationCopyFailed { .. } => StatusTone::Error,
        }
    }

    pub(super) fn status(&self) -> String {
        match &self.state {
            LastActionState::Ready => "Ready".to_owned(),
            LastActionState::Completed { action, completion } => match completion {
                CommandCompletion::Succeeded { .. } => match action {
                    LauncherAction::LaunchStudio => {
                        "Studio opened; sign in inside Studio if asked".to_owned()
                    }
                    LauncherAction::BrowserSignIn => {
                        "Studio opened; browser sign-in requested".to_owned()
                    }
                    LauncherAction::InstallStudio
                    | LauncherAction::CheckSetup
                    | LauncherAction::SaveSettings
                    | LauncherAction::CheckAiConnection => {
                        format!("{} completed successfully", action.label())
                    }
                },
                CommandCompletion::Failed { exit_code, .. } => format!(
                    "{} failed with {}",
                    action.label(),
                    exit_code.map_or_else(|| "no exit code".to_owned(), |code| code.to_string())
                ),
                CommandCompletion::CouldNotStart { .. } => {
                    format!("{} could not start", action.label())
                }
                CommandCompletion::CouldNotCollect { .. } => {
                    format!("{} could not be collected", action.label())
                }
            },
            LastActionState::CouldNotFindLauncher { action, .. } => {
                format!("{} could not start", action.label())
            }
            LastActionState::DiagnosticCompleted { action, .. } => {
                format!("{} completed", action.label())
            }
            LastActionState::WorkerDisconnected { action } => {
                format!("{} stopped unexpectedly", action.label())
            }
            LastActionState::ConfigurationCopied { .. } => "AI setup copied".to_owned(),
            LastActionState::ConfigurationCopyFailed { .. } => {
                "Could not create AI setup".to_owned()
            }
        }
    }

    pub(super) fn details(&self) -> String {
        match &self.state {
            LastActionState::Ready => {
                "Use Check setup to find anything that needs attention.".to_owned()
            }
            LastActionState::Completed { completion, .. } => completion.diagnostics(),
            LastActionState::DiagnosticCompleted { diagnostics, .. } => diagnostics.clone(),
            LastActionState::CouldNotFindLauncher { message, .. }
            | LastActionState::ConfigurationCopyFailed { message } => message.clone(),
            LastActionState::WorkerDisconnected { action } => {
                format!(
                    "The {} worker stopped before returning a result.",
                    action.label()
                )
            }
            LastActionState::ConfigurationCopied { configuration } => configuration.clone(),
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
