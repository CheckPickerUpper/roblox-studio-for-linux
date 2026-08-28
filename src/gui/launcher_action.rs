#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LauncherAction {
    LaunchStudio,
    InstallStudio,
    CheckSetup,
    BrowserSignIn,
    SaveSettings,
    CheckAiConnection,
}

impl LauncherAction {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::LaunchStudio => "Launch Studio",
            Self::InstallStudio => "Install / update Studio",
            Self::CheckSetup => "Check setup",
            Self::BrowserSignIn => "Browser sign-in",
            Self::SaveSettings => "Save sign-in setting",
            Self::CheckAiConnection => "Check AI connection",
        }
    }

    pub(super) const fn minimizes_launcher_after_success(self) -> bool {
        matches!(self, Self::LaunchStudio | Self::BrowserSignIn)
    }
}
