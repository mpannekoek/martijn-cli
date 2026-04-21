use std::error::Error;
use std::io::{self, Write};
use std::process::Stdio;

use clap::{Parser, Subcommand};
use figlet_rs::FIGfont;
use owo_colors::OwoColorize;
use tokio::process::Command;

type AppResult<T> = Result<T, Box<dyn Error>>;

#[derive(Parser, Debug)]
#[command(name = "martijn")]
#[command(about = "A personal CLI workspace for automation, tooling, and future utilities created by Martijn Pannekoek")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Open the interactive Azure auth shell.
    Azure,
    /// Open a minimal interactive dummy shell.
    Dummy,
}

#[derive(Debug, Default)]
struct SessionState {
    account: Option<AzureAccount>,
}

#[derive(Debug)]
struct AzureAccount {
    name: String,
    subscription_id: String,
    user: String,
}

#[tokio::main]
async fn main() -> AppResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Azure) => run_azure_shell().await?,
        Some(Commands::Dummy) => run_dummy_shell().await?,
        None => run_root_shell().await?,
    }

    Ok(())
}

async fn run_root_shell() -> AppResult<()> {
    print_root_intro();

    loop {
        print!("martijn> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        match input.trim() {
            "" => {}
            "help" => print_root_help(),
            "/azure" => run_azure_shell().await?,
            "/dummy" => run_dummy_shell().await?,
            "exit" | "quit" => {
                println!("Closing shell.");
                break;
            }
            other => println!("Unknown command `{other}`. Type `help` to see available commands."),
        }
    }

    Ok(())
}

fn print_root_intro() {
    print_root_banner();
    println!(
        "{}",
        "Welcome to Martijn CLI. Ready when you are. 🚀"
            .bold()
            .bright_white()
    );
    println!(
        "{}",
        "Launch a shell with `/azure` or `/dummy`, or type `help` to see the available commands."
            .bright_blue()
    );
    println!();
}

fn print_root_banner() {
    let banner = if let Ok(font) = FIGfont::standard() {
        font.convert("MARTIJN CLI").map(|figure| figure.to_string())
    } else {
        None
    };

    match banner {
        Some(text) if !text.trim().is_empty() => {
            for line in text.lines() {
                println!("{}", line.bright_cyan().bold());
            }
        }
        _ => println!("{}", "MARTIJN CLI".bold().bright_cyan()),
    }
}

async fn run_azure_shell() -> AppResult<()> {
    let mut state = SessionState::default();
    refresh_session_state(&mut state).await;

    println!("Interactive Azure shell");
    println!("Type `help` to see available commands.");
    print_status(&state);

    loop {
        print!("martijn> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        match input.trim() {
            "" => {}
            "help" => print_help(),
            "status" => {
                refresh_session_state(&mut state).await;
                print_status(&state);
            }
            "login" => {
                match run_az_command(&["login"]).await {
                    Ok(true) => println!("Login completed."),
                    Ok(false) => println!("`az login` did not complete successfully."),
                    Err(error) => println!("Unable to run `az login`: {error}"),
                }

                refresh_session_state(&mut state).await;
                print_status(&state);
            }
            "logout" => {
                match run_az_command(&["logout"]).await {
                    Ok(true) => println!("Logged out of Azure CLI."),
                    Ok(false) => println!("`az logout` did not complete successfully."),
                    Err(error) => println!("Unable to run `az logout`: {error}"),
                }

                refresh_session_state(&mut state).await;
                print_status(&state);
            }
            "exit" | "quit" => {
                println!("Closing shell.");
                break;
            }
            other => println!("Unknown command `{other}`. Type `help` to see available commands."),
        }
    }

    Ok(())
}

async fn run_dummy_shell() -> AppResult<()> {
    println!("Interactive Dummy shell");
    println!("Type `help` to see available commands.");

    loop {
        print!("martijn> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        let trimmed = input.trim();

        if trimmed.is_empty() {
            continue;
        }

        match trimmed {
            "help" => print_dummy_help(),
            "ping" => println!("pong"),
            "exit" | "quit" => {
                println!("Closing shell.");
                break;
            }
            _ => {
                if let Some(text) = trimmed.strip_prefix("echo ") {
                    println!("{text}");
                } else if trimmed == "echo" {
                    println!();
                } else {
                    println!(
                        "Unknown command `{trimmed}`. Type `help` to see available commands."
                    );
                }
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!("Available commands:");
    println!("  login   Run `az login`");
    println!("  logout  Run `az logout`");
    println!("  status  Show the current Azure login state");
    println!("  help    Show this help message");
    println!("  exit    Close the shell");
}

fn print_root_help() {
    println!("Available commands:");
    println!("  /azure  Open the Azure shell");
    println!("  /dummy  Open the Dummy shell");
    println!("  help    Show this help message");
    println!("  exit    Close the shell");
}

fn print_dummy_help() {
    println!("Available commands:");
    println!("  ping         Print `pong`");
    println!("  echo <text>  Print the provided text");
    println!("  help         Show this help message");
    println!("  exit         Close the shell");
}

fn print_status(state: &SessionState) {
    match &state.account {
        Some(account) => {
            println!(
                "Logged in as {} ({}) on subscription {}",
                account.user, account.name, account.subscription_id
            );
        }
        None => println!("Not logged in to Azure CLI."),
    }
}

async fn refresh_session_state(state: &mut SessionState) {
    match fetch_azure_account().await {
        Ok(account) => state.account = account,
        Err(error) => {
            state.account = None;
            println!("Azure status check failed: {error}");
        }
    }
}

async fn fetch_azure_account() -> AppResult<Option<AzureAccount>> {
    let output = Command::new("az")
        .args([
            "account",
            "show",
            "--query",
            "[name, id, user.name]",
            "--output",
            "tsv",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|error| format!("`az` is not available: {error}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let mut fields = raw.lines().map(str::trim).filter(|field| !field.is_empty());

    let Some(name) = fields.next() else {
        return Ok(None);
    };
    let Some(subscription_id) = fields.next() else {
        return Ok(None);
    };
    let Some(user) = fields.next() else {
        return Ok(None);
    };

    Ok(Some(AzureAccount {
        name: name.to_owned(),
        subscription_id: subscription_id.to_owned(),
        user: user.to_owned(),
    }))
}

async fn run_az_command(args: &[&str]) -> AppResult<bool> {
    let status = Command::new("az")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|error| format!("`az` is not available: {error}"))?;

    Ok(status.success())
}
