// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this example shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};

// Start the dummy shell, which acts as a small learning-oriented example.
pub(crate) async fn run() -> AppResult<()> {
    // This shell has no persistent state, so the unit type `()` is enough.
    let state = ();
    // Reuse the shared shell engine with this shell's intro and command handler.
    engine::run_shell(state, print_intro, handle_command).await
}

// Print the intro for the dummy shell.
// The `&()` argument is unused because this shell stores no state.
fn print_intro(_: &()) {
    // Identify the shell the user is currently in.
    println!("Interactive Dummy shell");
    // Point the user to the help command for discoverability.
    println!("Type `help` to see available commands.");
}

// Handle one command entered in the dummy shell.
fn handle_command<'a>(_: &'a mut (), input: &'a str) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Match on the exact command text after trimming.
        match input {
            "help" => {
                // Print the help text for the dummy shell.
                print_help();
            }
            "ping" => {
                // Reply with `pong` so users can test that the shell is responsive.
                println!("pong");
            }
            "exit" | "quit" => {
                // Tell the user that the shell is about to close.
                println!("Closing shell.");
                // Ask the shared shell engine to stop the loop.
                return Ok(ShellAction::Exit);
            }
            _ => {
                // Treat any other input as a possible `echo` command.
                if let Some(text) = input.strip_prefix("echo ") {
                    // Print the text that comes after `echo `.
                    println!("{text}");
                } else if input == "echo" {
                    // Print an empty line when the user entered `echo` without text.
                    println!();
                } else {
                    // Explain that the command was unknown and point to `help`.
                    println!("Unknown command `{input}`. Type `help` to see available commands.");
                }
            }
        }

        // Keep the shell open after every non-exit command.
        Ok(ShellAction::Continue)
    })
}

// Print the list of commands supported by the dummy shell.
fn print_help() {
    // Start with a heading so the help output is easy to scan.
    println!("Available commands:");
    // Explain the `ping` example command.
    println!("  ping         Print `pong`");
    // Explain how the `echo` command works.
    println!("  echo <text>  Print the provided text");
    // Explain how to show help again.
    println!("  help         Show this help message");
    // Explain how to close the shell.
    println!("  exit         Close the shell");
}
