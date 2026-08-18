// @error-boundary

mod cli;
mod config;
mod deployment;
mod desktop;
mod error;
mod runtime;

const LAUNCHER_ERROR_EXIT_CODE: i32 = 1;

fn main() {
    tracing_subscriber::fmt()
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
    std::process::exit(exit_code);
}
