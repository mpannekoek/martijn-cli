// Declare the `cli` module so this file can use the parsed command-line types.
mod cli;
// Declare grouped command handlers for the non-interactive CLI.
mod commands;
// Declare the temporary interactive start screen shown without commands.
mod interactive;
// Declare the shared config module so commands and services can read `config.toml`.
mod config;
// Declare the `azure` module so commands can delegate Azure-specific service logic.
mod azure;

// Bring the standard error trait into scope so we can erase concrete error types.
use std::error::Error;

// Bring Clap's `Parser` trait into scope so `Cli::parse()` becomes available.
use clap::Parser;

// Import the top-level CLI struct and the enum that stores the chosen subcommand.
use crate::cli::{Cli, Commands};

// This alias keeps return types short while still allowing different error kinds.
pub(crate) type AppResult<T> = Result<T, Box<dyn Error>>;

// Start a Tokio runtime so `main` can await async command functions.
#[tokio::main]
async fn main() -> AppResult<()> {
    // Parse the raw command-line arguments into our strongly typed `Cli` struct.
    let cli = Cli::parse();

    // Decide which command path to run based on the optional subcommand.
    match cli.command {
        Some(Commands::Azure { command }) => {
            // Run the requested Azure task as a normal non-interactive command.
            commands::azure::run_command(command).await?;
        }
        Some(Commands::Dummy { command }) => {
            // Run the requested dummy task as a normal non-interactive command.
            commands::dummy::run_command(command)?;
        }
        None => {
            // Show only the lightweight interactive start screen when no command is provided.
            interactive::print_start_screen();
        }
    }

    // Return success after the selected path finishes cleanly.
    Ok(())
}
