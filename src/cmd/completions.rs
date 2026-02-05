use crate::Cli;
use anyhow::Result;
use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate, Shell};
use std::io;

#[derive(Clone, Debug, ValueEnum)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

impl From<ShellType> for Shell {
    fn from(shell: ShellType) -> Shell {
        match shell {
            ShellType::Bash => Shell::Bash,
            ShellType::Zsh => Shell::Zsh,
            ShellType::Fish => Shell::Fish,
            ShellType::PowerShell => Shell::PowerShell,
            ShellType::Elvish => Shell::Elvish,
        }
    }
}

pub fn run(shell: &ShellType) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(
        Shell::from(shell.clone()),
        &mut cmd,
        bin_name,
        &mut io::stdout(),
    );
    Ok(())
}
