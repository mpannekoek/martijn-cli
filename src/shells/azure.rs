// Import Clap helpers so this shell can describe its interactive command model.
use clap::{Parser, Subcommand};
// Import `Stdio` so we can control how spawned processes use standard streams.
use std::process::Stdio;
// Import Tokio's async `Command` so Azure CLI calls work inside async code.
use tokio::process::Command;
// Import `Uuid` so we can try parse the tenant identifier as a UUID for better error messages.
use uuid::Uuid;
// Import terminal color helpers so the intro stands out.
use owo_colors::OwoColorize;

// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};

// Keep the Azure shell's mutable state in one place.
#[derive(Debug, Default)]
struct SessionState {
    // Store the currently detected Azure account.
    // We use `Option` because the user may not be logged in.
    account: Option<AzureAccount>,
}

// Hold the account details that the shell shows in `status`.
#[derive(Debug)]
struct AzureAccount {
    // Store the friendly subscription name returned by Azure CLI.
    name: String,
    // Store the active subscription identifier.
    subscription_id: String,
    // Store the current Azure user or service principal name.
    user: String,
}

// Describe the argument shape for one Azure-shell command line.
#[derive(Parser, Debug)]
#[command(
    name = "azure",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct AzureShellCli {
    // Store the one subcommand that the user typed in the Azure shell.
    #[command(subcommand)]
    command: AzureCommand,
}

// List the commands that the Azure shell understands.
#[derive(Subcommand, Debug)]
enum AzureCommand {
    /// Login to Azure CLI with the specified tenant id.
    Login {
        // Store the tenant id as an owned `String` because Clap creates owned values.
        tenant: String,
    },
    /// Run `az logout`.
    Logout,
    /// Show the current Azure login state.
    Status,
    /// Show the Azure shell help message.
    Help,
    /// Close the current shell session.
    #[command(alias = "quit")]
    Exit,
}

// Start the Azure shell.
pub(crate) async fn run() -> AppResult<()> {
    // Create fresh state with no cached account information yet.
    let mut state = SessionState::default();
    // Populate the state once before the shell starts so the intro shows real status.
    refresh_session_state(&mut state).await;

    // Reuse the shared shell engine with the Azure-specific intro and handler.
    engine::run_shell(state, print_intro, handle_command).await
}

// Handle one tokenized command entered in the Azure shell.
fn handle_command<'a>(state: &'a mut SessionState, tokens: &'a [String]) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Parse the shell tokens through Clap so commands and arguments stay typed.
        match parse_command(tokens) {
            Ok(AzureCommand::Help) => {
                // Print the Azure shell help text.
                engine::print_shell_help::<AzureShellCli>()?;
            }
            Ok(AzureCommand::Status) => {
                // Refresh the cached account information before showing it.
                refresh_and_print_status(state).await;
            }
            Ok(AzureCommand::Login { tenant }) => {
                // Run the login flow with the required tenant and then show the updated status.
                handle_login(state, &tenant).await;
            }
            Ok(AzureCommand::Logout) => {
                // Run the logout flow and then show the updated status.
                handle_logout(state).await;
            }
            Ok(AzureCommand::Exit) => {
                // Tell the user that the shell is about to close.
                println!("Closing shell.");
                // Ask the shared shell engine to stop the loop.
                return Ok(ShellAction::Exit);
            }
            Err(error) => {
                // Reuse the shared parse error printer so every shell responds consistently.
                engine::print_parse_error(error);
            }
        }

        // Keep the Azure shell open after every non-exit command.
        Ok(ShellAction::Continue)
    })
}

// Print the intro for the Azure shell.
fn print_intro(state: &SessionState) {
    // Identify the shell the user is currently in.
    println!("{}", "Interactive Azure shell".bright_cyan());
    // Point the user to the help command for discoverability.
    println!("{}", "Type `help` to see available commands.".bright_yellow());
    // Show the current login status immediately.
    print_status(state);
}

// Convert tokenized Azure-shell input into one typed command.
fn parse_command(tokens: &[String]) -> Result<AzureCommand, clap::Error> {
    // Reuse the shared helper so every shell performs the same Clap parsing steps.
    let cli = engine::parse_shell_command::<AzureShellCli>("azure", tokens)?;
    // Return only the subcommand because that is all the handler needs.
    Ok(cli.command)
}

// Print either the current Azure account or a message that no account is active.
fn print_status(state: &SessionState) {
    // Match on the optional account because the user may or may not be logged in.
    match &state.account {
        Some(account) => {
            // Show the user, subscription name and subscription identifier.
            println!(
                "Logged in as {} ({}) on subscription {}",
                account.user, account.name, account.subscription_id
            );
        }
        None => {
            // Explain clearly that no Azure login session was detected.
            println!("Not logged in to Azure CLI.");
        }
    }
}

// Run `az login`, report the outcome and refresh the visible state afterwards.
async fn handle_login(state: &mut SessionState, tenant: &str) {
    if !is_guid(tenant) {
        println!("Invalid tenant ID format. Please provide a valid UUID.");
        return;
    }

    // Run the Azure CLI login command and inspect whether it succeeded.
    match run_az_command(&["login", "--tenant", tenant]).await {
        Ok(true) => {
            // Tell the user that Azure CLI reported a successful login.
            println!("Login completed.");
        }
        Ok(false) => {
            // Tell the user that the command ran but did not report success.
            println!("`az login` did not complete successfully.");
        }
        Err(error) => {
            // Show the concrete error when the process could not even be started.
            println!("Unable to run `az login`: {error}");
        }
    }

    // Refresh and print the cached status so the shell reflects the newest state.
    refresh_and_print_status(state).await;
}

// Run `az logout`, report the outcome and refresh the visible state afterwards.
async fn handle_logout(state: &mut SessionState) {
    // Run the Azure CLI logout command and inspect whether it succeeded.
    match run_az_command(&["logout"]).await {
        Ok(true) => {
            // Tell the user that Azure CLI reported a successful logout.
            println!("Logged out of Azure CLI.");
        }
        Ok(false) => {
            // Tell the user that the command ran but did not report success.
            println!("`az logout` did not complete successfully.");
        }
        Err(error) => {
            // Show the concrete error when the process could not even be started.
            println!("Unable to run `az logout`: {error}");
        }
    }

    // Refresh and print the cached status so the shell reflects the newest state.
    refresh_and_print_status(state).await;
}

// Refresh the cached Azure account and immediately print the visible status.
async fn refresh_and_print_status(state: &mut SessionState) {
    // Update the in-memory account data first.
    refresh_session_state(state).await;
    // Print the new status after the refresh.
    print_status(state);
}

// Refresh the cached Azure account state by asking Azure CLI for the current account.
async fn refresh_session_state(state: &mut SessionState) {
    // Fetch the account data and handle both success and failure explicitly.
    match fetch_azure_account().await {
        Ok(account) => {
            // Replace the cached account with the freshly fetched value.
            state.account = account;
        }
        Err(error) => {
            // Clear the cached account when the status check itself failed.
            state.account = None;
            // Show the concrete error so the user understands why status is unavailable.
            println!("Azure status check failed: {error}");
        }
    }
}

// Ask Azure CLI for the active account and convert the output into structured data.
async fn fetch_azure_account() -> AppResult<Option<AzureAccount>> {
    // Build and run `az account show` with a query that only returns the fields we need.
    let output = azure_cli_command()
        .args([
            "account",
            "show",
            "--query",
            "[name, id, user.name]",
            "--output",
            "tsv",
        ])
        // Capture standard output because we need to parse the command result.
        .stdout(Stdio::piped())
        // Hide standard error because a failing status is enough to mean "not logged in".
        .stderr(Stdio::null())
        // Spawn the child process and wait for it to finish.
        .output()
        .await
        // Convert process startup errors into a readable application error.
        .map_err(|error| format!("`az` is not available: {error}"))?;

    // Treat a non-zero exit status as "no active account" instead of a hard failure.
    if !output.status.success() {
        return Ok(None);
    }

    // Decode the captured bytes into text, tolerating invalid UTF-8 if necessary.
    let raw_output = String::from_utf8_lossy(&output.stdout);
    // Parse the text into a strongly typed `AzureAccount`, if the format looks valid.
    let account = parse_account_from_tsv(raw_output.as_ref());

    // Return the parsed account, or `None` when the output was incomplete.
    Ok(account)
}

// Parse the three lines returned by our TSV query into an `AzureAccount`.
fn parse_account_from_tsv(raw_output: &str) -> Option<AzureAccount> {
    // Collect the non-empty trimmed lines into a vector we can inspect safely.
    let mut fields: Vec<&str> = Vec::new();

    // Walk over each line because the Azure CLI query returns one value per line.
    for line in raw_output.lines() {
        // Trim surrounding whitespace so accidental spacing does not break parsing.
        let trimmed = line.trim();
        // Skip empty lines because they do not hold useful field data.
        if trimmed.is_empty() {
            continue;
        }
        // Store the cleaned field for later validation.
        fields.push(trimmed);
    }

    // Require exactly three fields: subscription name, subscription id and user.
    if fields.len() != 3 {
        return None;
    }

    // Clone the first field into an owned `String` for the struct.
    let name = fields[0].to_owned();
    // Clone the second field into an owned `String` for the struct.
    let subscription_id = fields[1].to_owned();
    // Clone the third field into an owned `String` for the struct.
    let user = fields[2].to_owned();

    // Return the fully parsed account data.
    Some(AzureAccount {
        name,
        subscription_id,
        user,
    })
}

// Run an Azure CLI command while attaching it to the current terminal session.
async fn run_az_command(args: &[&str]) -> AppResult<bool> {
    // Start from the platform-correct Azure CLI executable name.
    let status = azure_cli_command()
        // Pass through the command-specific arguments such as `login` or `logout`.
        .args(args)
        // Reuse the current terminal input so interactive login can ask questions.
        .stdin(Stdio::inherit())
        // Reuse the current terminal output so the user sees Azure CLI output live.
        .stdout(Stdio::inherit())
        // Reuse the current terminal error stream for visible failures.
        .stderr(Stdio::inherit())
        // Run the process and wait for it to finish.
        .status()
        .await
        // Convert process startup errors into a readable application error.
        .map_err(|error| format!("`az` is not available: {error}"))?;

    // Return `true` when the command exited successfully.
    Ok(status.success())
}

// Build a `Command` with the correct Azure CLI executable name for this platform.
fn azure_cli_command() -> Command {
    #[cfg(windows)]
    {
        // Azure CLI commonly installs `az` as `az.cmd` on Windows.
        // Rust does not automatically resolve `.cmd` when the extension is omitted.
        Command::new("az.cmd")
    }

    #[cfg(not(windows))]
    {
        // Unix-like systems usually expose Azure CLI directly as `az`.
        Command::new("az")
    }
}

fn is_guid(s: &str) -> bool {
    // Ask the `uuid` crate to validate whether the text is a well-formed UUID.
    Uuid::try_parse(s).is_ok()
}

#[cfg(test)]
mod tests {
    // Import the Azure parser helper so the tests can validate command shapes.
    use super::{AzureCommand, parse_command};

    #[test]
    fn parses_login_with_one_tenant() {
        // Parse a valid login command with exactly one tenant argument.
        let parsed_command = parse_command(&[String::from("login"), String::from("my-tenant-id")])
            .expect("command should parse");

        // Confirm that Clap keeps the tenant value and returns the login variant.
        assert!(matches!(
            parsed_command,
            AzureCommand::Login { tenant } if tenant == "my-tenant-id"
        ));
    }

    #[test]
    fn rejects_login_without_a_tenant() {
        // Parse a login command that forgot the required tenant argument.
        let error = parse_command(&[String::from("login")]).expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports the missing required argument.
        assert!(rendered_error.contains("<TENANT>"));
    }

    #[test]
    fn rejects_login_with_extra_arguments() {
        // Parse a login command that provides more than one value after `login`.
        let error = parse_command(&[
            String::from("login"),
            String::from("tenant-one"),
            String::from("tenant-two"),
        ])
        .expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap reports the unexpected extra argument.
        assert!(rendered_error.contains("unexpected argument"));
    }

    #[test]
    fn parses_help_as_a_real_command() {
        // Parse the explicit help command that users can type inside the shell.
        let parsed_command = parse_command(&[String::from("help")]).expect("command should parse");

        // Confirm that help is represented as its own typed variant.
        assert!(matches!(parsed_command, AzureCommand::Help));
    }
}
