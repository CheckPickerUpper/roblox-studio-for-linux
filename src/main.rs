// @error-boundary

mod cli;
mod config;
mod error;
mod runtime;

fn main() {
    let exit_code = match cli::run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    };
    std::process::exit(exit_code);
}
