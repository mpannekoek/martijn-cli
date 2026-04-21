use std::process::Stdio;

use tokio::process::Command;

use crate::shells::engine::{self, CommandFuture, ShellAction};
use crate::AppResult;

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

pub(crate) async fn run() -> AppResult<()> {
    let mut state = SessionState::default();
    refresh_session_state(&mut state).await;

    engine::run_shell(state, print_intro, handle_command).await
}

fn handle_command<'a>(state: &'a mut SessionState, input: &'a str) -> CommandFuture<'a> {
    Box::pin(async move {
        match input {
            "help" => print_help(),
            "status" => {
                refresh_session_state(state).await;
                print_status(state);
            }
            "login" => {
                match run_az_command(&["login"]).await {
                    Ok(true) => println!("Login completed."),
                    Ok(false) => println!("`az login` did not complete successfully."),
                    Err(error) => println!("Unable to run `az login`: {error}"),
                }

                refresh_session_state(state).await;
                print_status(state);
            }
            "logout" => {
                match run_az_command(&["logout"]).await {
                    Ok(true) => println!("Logged out of Azure CLI."),
                    Ok(false) => println!("`az logout` did not complete successfully."),
                    Err(error) => println!("Unable to run `az logout`: {error}"),
                }

                refresh_session_state(state).await;
                print_status(state);
            }
            "exit" | "quit" => {
                println!("Closing shell.");
                return Ok(ShellAction::Exit);
            }
            other => println!("Unknown command `{other}`. Type `help` to see available commands."),
        }

        Ok(ShellAction::Continue)
    })
}

fn print_intro(state: &SessionState) {
    println!("Interactive Azure shell");
    println!("Type `help` to see available commands.");
    print_status(state);
}

fn print_help() {
    println!("Available commands:");
    println!("  login   Run `az login`");
    println!("  logout  Run `az logout`");
    println!("  status  Show the current Azure login state");
    println!("  help    Show this help message");
    println!("  exit    Close the shell");
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
    let output = azure_cli_command()
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
    let status = azure_cli_command()
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|error| format!("`az` is not available: {error}"))?;

    Ok(status.success())
}

fn azure_cli_command() -> Command {
    #[cfg(windows)]
    {
        // Azure CLI commonly installs `az` as `az.cmd` on Windows, and
        // Rust does not resolve non-.exe extensions when the extension is omitted.
        Command::new("az.cmd")
    }

    #[cfg(not(windows))]
    {
        Command::new("az")
    }
}
