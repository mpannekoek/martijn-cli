// Import Clap derive helpers so we can turn Rust types into CLI definitions.
use clap::{Parser, Subcommand};

// Derive `Parser` so Clap can fill this struct from command-line arguments.
#[derive(Parser, Debug)]
// Set the binary name shown in help output.
#[command(name = "martijn")]
// Describe the application in `--help` output.
#[command(
    about = "A personal CLI workspace for automation, tooling, and future utilities created by Martijn Pannekoek"
)]
pub(crate) struct Cli {
    // Store the selected subcommand, if the user supplied one.
    // We use `Option` because running the binary without a subcommand is valid.
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

// Derive `Subcommand` so each enum variant becomes a valid CLI subcommand.
#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Start the Azure shell that works with Azure CLI login state.
    Azure,
    /// Start the minimal dummy shell that is useful for learning and testing.
    Dummy,
}
