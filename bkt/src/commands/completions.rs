//! Shell completion generation.
//!
//! Generate completion scripts for various shells.

use anyhow::Result;
use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::Generator;
use clap_complete_nushell::Nushell;
use std::io;

use crate::cli::Cli;

/// Supported shell types for completion generation.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// Nushell
    Nushell,
}

#[derive(Debug, Parser)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: Shell,
}

/// Generate completions for the given shell and write to stdout.
fn print_completions<G: Generator>(generator: G, cmd: &mut clap::Command) {
    clap_complete::generate(
        generator,
        cmd,
        cmd.get_name().to_string(),
        &mut io::stdout(),
    );
}

pub fn run(args: CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();

    match args.shell {
        Shell::Bash => {
            print_completions(clap_complete::Shell::Bash, &mut cmd);
        }
        Shell::Zsh => {
            print_completions(clap_complete::Shell::Zsh, &mut cmd);
        }
        Shell::Fish => {
            print_completions(clap_complete::Shell::Fish, &mut cmd);
        }
        Shell::Nushell => {
            print_completions(Nushell, &mut cmd);
        }
    }

    Ok(())
}
