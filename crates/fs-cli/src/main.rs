//! `frankensim` command-line entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    let output = fs_cli::run_os(std::env::args_os().skip(1));
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    ExitCode::from(output.exit_code)
}
