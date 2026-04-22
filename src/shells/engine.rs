// Import `Future` because command handlers return asynchronous work.
use std::future::Future;
// Import input and output types used to read from stdin and flush the prompt.
use std::io::{self, Write};
// Import `Pin` because boxed futures need a stable memory location.
use std::pin::Pin;

// Import the shared application result type.
use crate::AppResult;

// This enum tells the shell loop what to do after one command finishes.
pub(crate) enum ShellAction {
    // Keep the shell open and show the prompt again.
    Continue,
    // Stop the shell loop and return to the caller.
    Exit,
}

// This alias gives the boxed async command future a short, readable name.
// The lifetime `'a` ties the future to the borrowed state and input.
pub(crate) type CommandFuture<'a> = Pin<Box<dyn Future<Output = AppResult<ShellAction>> + 'a>>;

// Run the shared read-eval-print loop for any shell in this application.
pub(crate) async fn run_shell<State, Intro, Handler>(
    // Keep shell-specific state mutable so commands can update it over time.
    mut state: State,
    // Accept a callback that prints the shell-specific introduction once.
    mut print_intro: Intro,
    // Accept a callback that handles one command at a time.
    mut handle_command: Handler,
) -> AppResult<()>
where
    // The intro callback only needs read-only access to the current state.
    Intro: FnMut(&State),
    // The handler borrows state and input for exactly as long as its future lives.
    Handler: for<'a> FnMut(&'a mut State, &'a str) -> CommandFuture<'a>,
{
    // Show the introduction before the command loop starts.
    print_intro(&state);

    // Keep reading commands until the user exits or stdin closes.
    loop {
        // Print the prompt without a newline so the user's input stays on the same line.
        print!("martijn> ");
        // Flush stdout so the prompt becomes visible immediately.
        io::stdout().flush()?;

        // Allocate a fresh buffer for the next line of input.
        let mut input = String::new();
        // Read one line from standard input into the buffer.
        let bytes_read = io::stdin().read_line(&mut input)?;
        // A read of `0` bytes means stdin reached end-of-file.
        if bytes_read == 0 {
            // Print a newline so the terminal stays visually tidy.
            println!();
            // Leave the shell loop because no more input is available.
            break;
        }

        // Remove surrounding whitespace so commands like `help   ` still work.
        let trimmed = input.trim();
        // Ignore empty input so pressing Enter does not trigger an error message.
        if trimmed.is_empty() {
            continue;
        }

        // Ask the shell-specific handler what to do with this command.
        match handle_command(&mut state, trimmed).await? {
            ShellAction::Continue => {
                // Do nothing special here because the loop will print the next prompt.
            }
            ShellAction::Exit => {
                // Stop the loop when the handler explicitly asks to exit.
                break;
            }
        }
    }

    // Return success once the shell loop ends cleanly.
    Ok(())
}
