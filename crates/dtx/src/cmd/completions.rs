//! Generate shell completions.

use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::io;

/// Run the completions command.
pub fn run(shell: Shell) {
    let mut cmd = crate::Cli::command();
    generate(shell, &mut cmd, "dtx", &mut io::stdout());
}
