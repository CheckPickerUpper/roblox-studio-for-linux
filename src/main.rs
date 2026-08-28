// @error-boundary

mod cli;
mod config;
mod deployment;
mod desktop;
mod error;
mod gui;
mod mcp;
mod runtime;

const LAUNCHER_ERROR_EXIT_CODE: i32 = 1;
const FLATPAK_STATUS_PATH_ENVIRONMENT: &str = "ROBLOX_LAUNCHER_FLATPAK_STATUS_PATH";

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_ansi(false)
        .init();
    let exit_code = match cli::run_launcher() {
        Ok(code) => code,
        Err(error) => {
            tracing::error!(error = %error, "Launcher failed");
            LAUNCHER_ERROR_EXIT_CODE
        }
    };
    if let Some(path) = std::env::var_os(FLATPAK_STATUS_PATH_ENVIRONMENT) {
        let _ = std::fs::write(path, exit_code.to_string());
    }
    std::process::exit(exit_code);
}
