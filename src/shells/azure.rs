// Import `Stdio` so we can control how spawned processes use standard streams.
use std::process::Stdio;

// Import Tokio's async `Command` so Azure CLI calls work inside async code.
use tokio::process::Command;

// Import `Uuid` so we can try parse the tenant identifier as a UUID for better error messages.
use uuid::Uuid;

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

// Represent one parsed command from the interactive Azure shell.
#[derive(Debug, PartialEq, Eq)]
enum ParsedCommand<'a> {
    // Show the Azure shell help output.
    Help,
    // Refresh and print the current Azure login state.
    Status,
    // Run Azure login with one required tenant value.
    Login {
        // Borrow the tenant text directly from the input line.
        tenant: &'a str,
    },
    // Run Azure logout.
    Logout,
    // Leave the interactive Azure shell.
    Exit,
    // Report that `login` was used without the required tenant value.
    LoginMissingTenant,
    // Report that `login` received unexpected extra arguments.
    LoginTooManyArguments {
        // Keep the extra arguments so the error can mention them back to the user.
        extra_arguments: Vec<&'a str>,
    },
    // Keep the original input for any command we do not recognize.
    Unknown {
        // Borrow the original input so we can echo it in the error message.
        input: &'a str,
    },
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

// Handle one command entered in the Azure shell.
fn handle_command<'a>(state: &'a mut SessionState, input: &'a str) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Parse the raw input first so commands with arguments are handled explicitly.
        match parse_command(input) {
            ParsedCommand::Help => {
                // Print the Azure shell help text.
                print_help();
            }
            ParsedCommand::Status => {
                // Refresh the cached account information before showing it.
                refresh_and_print_status(state).await;
            }
            ParsedCommand::Login { tenant } => {
                // Run the login flow with the required tenant and then show the updated status.
                handle_login(state, tenant).await;
            }
            ParsedCommand::Logout => {
                // Run the logout flow and then show the updated status.
                handle_logout(state).await;
            }
            ParsedCommand::Exit => {
                // Tell the user that the shell is about to close.
                println!("Closing shell.");
                // Ask the shared shell engine to stop the loop.
                return Ok(ShellAction::Exit);
            }
            ParsedCommand::LoginMissingTenant => {
                // Explain that `login` now needs one tenant argument.
                println!("The `login` command requires a tenant. Use `login <tenant>`.");
            }
            ParsedCommand::LoginTooManyArguments { extra_arguments } => {
                // Join the unexpected trailing arguments into one readable string.
                let extra_arguments = extra_arguments.join(" ");
                // Explain that only one tenant value is allowed for `login`.
                println!(
                    "The `login` command accepts exactly one tenant. Unexpected extra arguments: `{extra_arguments}`."
                );
            }
            ParsedCommand::Unknown { input } => {
                // Explain that the command was unknown and point to the help text.
                println!("Unknown command `{input}`. Type `help` to see available commands.");
            }
        }

        // Keep the Azure shell open after every non-exit command.
        Ok(ShellAction::Continue)
    })
}

// Print the intro for the Azure shell.
fn print_intro(state: &SessionState) {
    // Identify the shell the user is currently in.
    println!("Interactive Azure shell");
    // Point the user to the help command for discoverability.
    println!("Type `help` to see available commands.");
    // Show the current login status immediately.
    print_status(state);
}

// Print the list of commands supported by the Azure shell.
fn print_help() {
    // Start with a heading so the help output is easy to scan.
    println!("Available commands:");
    // Explain that login needs one tenant value and show the exact syntax.
    println!("  login <tenant-id>   Login to Azure CLI with the specified tenant`");
    // Explain how to log out from Azure CLI.
    println!("  logout              Run `az logout`");
    // Explain how to inspect the current login status.
    println!("  status              Show the current Azure login state");
    // Explain how to show help again.
    println!("  help                Show this help message");
    // Explain how to close the shell.
    println!("  exit                Close the shell");
}

// Parse one raw Azure shell input line into a structured command.
fn parse_command(input: &str) -> ParsedCommand<'_> {
    // Remove leading and trailing whitespace so spacing does not affect parsing.
    let trimmed_input = input.trim();

    // Return `Help` for an exact `help` command with no extra arguments.
    if trimmed_input == "help" {
        return ParsedCommand::Help;
    }

    // Return `Status` for an exact `status` command with no extra arguments.
    if trimmed_input == "status" {
        return ParsedCommand::Status;
    }

    // Return `Logout` for an exact `logout` command with no extra arguments.
    if trimmed_input == "logout" {
        return ParsedCommand::Logout;
    }

    // Return `Exit` for the standard shell exit words.
    if trimmed_input == "exit" || trimmed_input == "quit" {
        return ParsedCommand::Exit;
    }

    // Split the trimmed input on runs of whitespace so repeated spaces are harmless.
    let mut parts = trimmed_input.split_whitespace();
    // Read the first token because it tells us which command the user wanted.
    let Some(command_name) = parts.next() else {
        // Treat an empty line as unknown so the existing shell behavior stays simple.
        return ParsedCommand::Unknown {
            input: trimmed_input,
        };
    };

    // Handle commands that need argument-aware parsing.
    match command_name {
        "login" => {
            // Read the required tenant argument from the next token, if present.
            let Some(tenant) = parts.next() else {
                // Report the missing tenant explicitly so the user gets a helpful hint.
                return ParsedCommand::LoginMissingTenant;
            };

            // Collect any remaining tokens because `login` allows only one tenant value.
            let extra_arguments: Vec<&str> = parts.collect();

            // Reject extra values instead of silently ignoring them.
            if !extra_arguments.is_empty() {
                return ParsedCommand::LoginTooManyArguments { extra_arguments };
            }

            // Return the parsed login command with the borrowed tenant text.
            ParsedCommand::Login { tenant }
        }
        _ => {
            // Preserve the full trimmed input for unknown-command feedback.
            ParsedCommand::Unknown {
                input: trimmed_input,
            }
        }
    }
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
    Uuid::try_parse(s).is_ok()
}

#[cfg(test)]
mod tests {
    // Import the parsing helpers from the parent module so the tests can use them directly.
    use super::{ParsedCommand, parse_command};

    #[test]
    fn parses_login_with_one_tenant() {
        // Parse a valid login command with exactly one tenant argument.
        let parsed_command = parse_command("login my-tenant-id");

        // Confirm that the parser keeps the tenant value and returns the login variant.
        assert_eq!(
            parsed_command,
            ParsedCommand::Login {
                tenant: "my-tenant-id",
            }
        );
    }

    #[test]
    fn rejects_login_without_a_tenant() {
        // Parse a login command that forgot the required tenant argument.
        let parsed_command = parse_command("login");

        // Confirm that the parser reports the specific missing-tenant error.
        assert_eq!(parsed_command, ParsedCommand::LoginMissingTenant);
    }

    #[test]
    fn rejects_login_with_extra_arguments() {
        // Parse a login command that provides more than one value after `login`.
        let parsed_command = parse_command("login tenant-one tenant-two");

        // Confirm that the parser reports the unexpected trailing argument.
        assert_eq!(
            parsed_command,
            ParsedCommand::LoginTooManyArguments {
                extra_arguments: vec!["tenant-two"],
            }
        );
    }
}
