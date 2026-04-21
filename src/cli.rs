use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "martijn")]
#[command(about = "A personal CLI workspace for automation, tooling, and future utilities created by Martijn Pannekoek")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Open the interactive Azure auth shell.
    Azure,
    /// Open a minimal interactive dummy shell.
    Dummy,
}
