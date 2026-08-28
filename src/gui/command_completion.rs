use std::process::Output;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CommandCompletion {
    Succeeded {
        stdout: String,
        stderr: String,
    },
    Failed {
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    CouldNotStart {
        message: String,
    },
    CouldNotCollect {
        message: String,
    },
}

impl CommandCompletion {
    pub(super) fn from_output(output: Output) -> Self {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if output.status.success() {
            Self::Succeeded { stdout, stderr }
        } else {
            Self::Failed {
                exit_code: output.status.code(),
                stdout,
                stderr,
            }
        }
    }

    pub(super) const fn succeeded(&self) -> bool {
        matches!(self, Self::Succeeded { .. })
    }

    pub(super) fn stdout(&self) -> &str {
        match self {
            Self::Succeeded { stdout, .. } | Self::Failed { stdout, .. } => stdout,
            Self::CouldNotStart { .. } | Self::CouldNotCollect { .. } => "",
        }
    }

    pub(super) fn diagnostics(&self) -> String {
        match self {
            Self::Succeeded { stdout, stderr } | Self::Failed { stdout, stderr, .. } => {
                match (stdout.is_empty(), stderr.is_empty()) {
                    (true, true) => "The command did not produce any output.".to_owned(),
                    (false, true) => stdout.clone(),
                    (true, false) => stderr.clone(),
                    (false, false) => format!("{stdout}\n{stderr}"),
                }
            }
            Self::CouldNotStart { message } | Self::CouldNotCollect { message } => message.clone(),
        }
    }
}
