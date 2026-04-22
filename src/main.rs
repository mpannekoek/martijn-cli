// Declare the `cli` module so this file can use the parsed command-line types.
mod cli;
// Declare the `shells` module so `main` can route into the interactive shells.
mod shells;

// Bring the standard error trait into scope so we can erase concrete error types.
use std::error::Error;

// Bring Clap's `Parser` trait into scope so `Cli::parse()` becomes available.
use clap::Parser;

// Import the top-level CLI struct and the enum that stores the chosen subcommand.
use crate::cli::{Cli, Commands};

// This alias keeps return types short while still allowing different error kinds.
pub(crate) type AppResult<T> = Result<T, Box<dyn Error>>;

// Start a Tokio runtime so `main` can await async shell functions.
#[tokio::main]
async fn main() -> AppResult<()> {
    // Parse the raw command-line arguments into our strongly typed `Cli` struct.
    let cli = Cli::parse();

    // Decide which shell to start based on the optional subcommand.
    match cli.command {
        Some(Commands::Azure) => {
            // Run the Azure shell when the user asked for Azure-specific commands.
            shells::azure::run().await?;
        }
        Some(Commands::Dummy) => {
            // Run the dummy shell when the user wants the minimal example shell.
            shells::dummy::run().await?;
        }
        None => {
            // Fall back to the root shell when no subcommand was provided.
            shells::root::run().await?;
        }
    }

    // Return success after the selected shell exits cleanly.
    Ok(())
}
