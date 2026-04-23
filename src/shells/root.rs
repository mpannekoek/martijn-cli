// Import Clap helpers so this shell can describe its interactive command model.
use clap::{Parser, Subcommand};
// Import the FIGlet font loader used to build the banner text.
use figlet_rs::FIGfont;
// Import terminal color helpers so the intro stands out.
use owo_colors::OwoColorize;

// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this interactive shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};

// Keep one shared shell name so the prompt and Clap parser entry stay in sync.
const SHELL_NAME: &str = "martijn";

// Describe the argument shape for one root-shell command line.
#[derive(Parser, Debug)]
#[command(
    name = "martijn",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct RootShellCli {
    // Store the one subcommand that the user typed in the root shell.
    #[command(subcommand)]
    command: RootCommand,
}

// List the commands that the root shell understands.
#[derive(Subcommand, Debug)]
enum RootCommand {
    /// Open the Azure shell.
    Azure,
    /// Open the Dummy shell.
    Dummy,
    /// Show the root shell help message.
    Help,
    /// Close the current shell session.
    #[command(alias = "quit")]
    Exit,
}

// Start the root shell that lets users jump into more specific shells.
pub(crate) async fn run() -> AppResult<()> {
    // This shell does not need persistent state, so we use the unit type `()`.
    let state = ();
    // Reuse the shared shell engine with this shell's intro and command handler.
    engine::run_shell(state, print_root_intro, handle_command, SHELL_NAME).await
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
        "Launch a shell with `azure` or `dummy`, or type `help` to see the available commands."
            .bright_yellow()
    );
    // Add a blank line so the prompt does not touch the intro text.
    println!();
}

// Handle one tokenized command entered in the root shell.
fn handle_command<'a>(_: &'a mut (), tokens: &'a [String]) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Parse the shell tokens through Clap so subcommands and aliases stay typed.
        match parse_command(tokens) {
            Ok(RootCommand::Help) => {
                // Print the root help text when the user asks for guidance.
                engine::print_shell_help::<RootShellCli>()?;
            }
            Ok(RootCommand::Azure) => {
                // Enter the Azure shell and wait until that shell exits.
                super::azure::run().await?;
            }
            Ok(RootCommand::Dummy) => {
                // Enter the dummy shell and wait until that shell exits.
                super::dummy::run().await?;
            }
            Ok(RootCommand::Exit) => {
                // Tell the user that the shell is about to close.
                println!("Closing shell.");
                // Tell the shared engine to stop the loop.
                return Ok(ShellAction::Exit);
            }
            Err(error) => {
                // Reuse the shared parse error printer so every shell responds consistently.
                engine::print_parse_error(error);
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

// Convert tokenized root-shell input into one typed command.
fn parse_command(tokens: &[String]) -> Result<RootCommand, clap::Error> {
    // Reuse the shared helper so every shell performs the same Clap parsing steps.
    let cli = engine::parse_shell_command::<RootShellCli>(SHELL_NAME, tokens)?;
    // Return only the subcommand because that is all the handler needs.
    Ok(cli.command)
}

#[cfg(test)]
mod tests {
    // Import the root parser helper so the tests can validate command shapes.
    use super::{RootCommand, parse_command};

    #[test]
    fn parses_the_azure_subcommand() {
        // Parse the simplest valid root-shell command.
        let command = parse_command(&[String::from("azure")]).expect("command should parse");

        // Confirm that Clap routes the input to the Azure subcommand.
        assert!(matches!(command, RootCommand::Azure));
    }

    #[test]
    fn rejects_unknown_subcommands() {
        // Parse a command name that the root shell does not support.
        let error = parse_command(&[String::from("unknown")]).expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap clearly reports the invalid subcommand.
        assert!(rendered_error.contains("unrecognized subcommand"));
    }

    #[test]
    fn parses_help_as_a_real_command() {
        // Parse the explicit help command that users can type inside the shell.
        let command = parse_command(&[String::from("help")]).expect("command should parse");

        // Confirm that help is represented as its own typed variant.
        assert!(matches!(command, RootCommand::Help));
    }
}
