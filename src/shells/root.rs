// Import the FIGlet font loader used to build the banner text.
use figlet_rs::FIGfont;
// Import terminal color helpers so the intro stands out.
use owo_colors::OwoColorize;

// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this interactive shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};

// Start the root shell that lets users jump into more specific shells.
pub(crate) async fn run() -> AppResult<()> {
    // This shell does not need persistent state, so we use the unit type `()`.
    let state = ();
    // Reuse the shared shell engine with this shell's intro and command handler.
    engine::run_shell(state, print_root_intro, handle_command).await
}

// Print the intro for the root shell.
// The `&()` argument is unused because the root shell stores no state.
fn print_root_intro(_: &()) {
    // Show the banner first so the shell has a clear visual identity.
    print_root_banner();
    // Print a welcome line in bright white to make it easy to notice.
    println!(
        "{}",
        "Welcome to Martijn CLI. Ready when you are. 🚀"
            .bold()
            .bright_white()
    );
    // Explain how users can reach the child shells from here.
    println!(
        "{}",
        "Launch a shell with `/azure` or `/dummy`, or type `help` to see the available commands."
            .bright_blue()
    );
    // Add a blank line so the prompt does not touch the intro text.
    println!();
}

// Handle one command entered in the root shell.
fn handle_command<'a>(_: &'a mut (), input: &'a str) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Match on the exact trimmed command text.
        match input {
            "help" => {
                // Print the root help text when the user asks for guidance.
                print_help();
            }
            "/azure" => {
                // Enter the Azure shell and wait until that shell exits.
                super::azure::run().await?;
            }
            "/dummy" => {
                // Enter the dummy shell and wait until that shell exits.
                super::dummy::run().await?;
            }
            "exit" | "quit" => {
                // Tell the user that the shell is about to close.
                println!("Closing shell.");
                // Tell the shared engine to stop the loop.
                return Ok(ShellAction::Exit);
            }
            other => {
                // Mention the unknown command and suggest the recovery path.
                println!("Unknown command `{other}`. Type `help` to see available commands.");
            }
        }

        // Keep the root shell open after every non-exit command.
        Ok(ShellAction::Continue)
    })
}

// Render the root banner, using FIGlet when that succeeds.
fn print_root_banner() {
    // Ask the FIGlet crate for the standard built-in font.
    let font_result = FIGfont::standard();
    // Convert the font-loading result into optional banner text.
    let banner = match font_result {
        Ok(font) => {
            // Try to render the banner text with the loaded font.
            match font.convert("MARTIJN CLI") {
                Some(figure) => {
                    // Turn the rendered figure into an owned `String`.
                    Some(figure.to_string())
                }
                None => {
                    // Return `None` when the font could not render this text.
                    None
                }
            }
        }
        Err(_) => {
            // Return `None` when loading the font failed.
            None
        }
    };

    // Print the FIGlet banner when we have one, otherwise fall back to plain text.
    match banner {
        Some(text) if !text.trim().is_empty() => {
            // Print each line separately so the color styling stays consistent.
            for line in text.lines() {
                println!("{}", line.bright_cyan().bold());
            }
        }
        _ => {
            // Use a simple fallback banner when FIGlet output is unavailable.
            println!("{}", "MARTIJN CLI".bold().bright_cyan());
        }
    }
}

// Print the list of commands supported by the root shell.
fn print_help() {
    // Start with a heading so the help output is easy to scan.
    println!("Available commands:");
    // Explain how to open the Azure shell.
    println!("  /azure  Open the Azure shell");
    // Explain how to open the dummy shell.
    println!("  /dummy  Open the Dummy shell");
    // Explain how to reopen this help text.
    println!("  help    Show this help message");
    // Explain how to close the shell.
    println!("  exit    Close the shell");
}
