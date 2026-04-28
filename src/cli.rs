// Import Clap derive helpers so we can turn Rust types into CLI definitions.
use clap::{Parser, Subcommand};

// Import the Azure command tree so all Azure tasks are reachable from Clap.
use crate::commands::azure::AzureCommand;
// Import the dummy command tree so the example commands also stay available.
use crate::commands::dummy::DummyCommand;

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
    /// Run Azure-related tasks.
    #[command(arg_required_else_help = true)]
    Azure {
        // Store the selected Azure subcommand.
        #[command(subcommand)]
        command: AzureCommand,
    },
    /// Run minimal dummy tasks that are useful for learning and testing.
    #[command(arg_required_else_help = true)]
    Dummy {
        // Store the selected dummy subcommand.
        #[command(subcommand)]
        command: DummyCommand,
    },
}
