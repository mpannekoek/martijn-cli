// Declare the command parser module so this shell keeps Clap-specific types together.
mod commands;
// Declare the inventory module so report commands have a focused home.
mod inventory;
// Declare the login module so authentication flow and validation stay together.
mod login;
// Declare the state module so cached Azure account handling stays together.
mod state;

// Import the typed command model used by the shell router.
use commands::{AzureCommand, AzureShellCli, InventoryCommand, parse_command};
// Import inventory command handlers that do the user-facing report work.
use inventory::{handle_inventory_generate, handle_inventory_list};
// Import the login handler that resolves arguments and calls Azure CLI.
use login::{handle_login, handle_logout};
// Import terminal color helpers so the intro stands out.
use owo_colors::OwoColorize;
// Import state helpers used by startup, status and intro rendering.
use state::{SessionState, print_status, refresh_and_print_status, refresh_session_state};

// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};

// Keep one shared shell name so the prompt and Clap parser entry stay in sync.
const SHELL_NAME: &str = "azure";

// Start the Azure shell.
pub(crate) async fn run() -> AppResult<()> {
    // Create fresh state with no cached account information yet.
    let mut state = SessionState::default();
    // Populate the state once before the shell starts so the intro shows real status.
    refresh_session_state(&mut state).await;

    // Reuse the shared shell engine with the Azure-specific intro and handler.
    engine::run_shell(state, print_intro, handle_command, SHELL_NAME).await
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
            Ok(AzureCommand::Inventory(InventoryCommand::Generate)) => {
                // Build the Markdown inventory report and save it to disk.
                handle_inventory_generate(state).await;
            }
            Ok(AzureCommand::Inventory(InventoryCommand::List)) => {
                // List the Markdown inventory reports already saved on disk.
                handle_inventory_list();
            }
            Ok(AzureCommand::Login(arguments)) => {
                // Run the login flow after resolving CLI arguments and config defaults together.
                handle_login(state, &arguments).await;
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
                // Treat Clap's generated help as a successful user request, not as a mistake.
                if error.kind() == clap::error::ErrorKind::DisplayHelp
                    || error.kind()
                        == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                {
                    // Print the rendered help exactly as Clap produced it.
                    print!("{error}");
                } else {
                    // Reuse the shared parse error printer so real mistakes still get a hint.
                    engine::print_parse_error(error);
                }
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
    println!(
        "{}",
        "Type `help` to see available commands.".bright_yellow()
    );
    // Show the current login status immediately.
    print_status(state);
}
