mod cli;
mod shells;

use std::error::Error;

use clap::Parser;

use crate::cli::{Cli, Commands};

pub(crate) type AppResult<T> = Result<T, Box<dyn Error>>;

#[tokio::main]
async fn main() -> AppResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Azure) => shells::azure::run().await?,
        Some(Commands::Dummy) => shells::dummy::run().await?,
        None => shells::root::run().await?,
    }

    Ok(())
}
