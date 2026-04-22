// Import Clap helpers so this shell can describe its interactive command model.
use clap::{Parser, Subcommand};
// Import terminal color helpers so the intro stands out.
use owo_colors::OwoColorize;
// Import the shared application result type.
use crate::AppResult;
// Import the shared shell engine pieces used by this example shell.
use crate::shells::engine::{self, CommandFuture, ShellAction};

// Describe the argument shape for one dummy-shell command line.
#[derive(Parser, Debug)]
#[command(
    name = "dummy",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
struct DummyShellCli {
    // Store the one subcommand that the user typed in the dummy shell.
    #[command(subcommand)]
    command: DummyCommand,
}

// List the commands that the dummy shell understands.
#[derive(Subcommand, Debug)]
enum DummyCommand {
    /// Reply with `pong`.
    Ping,
    /// Print the provided text.
    Echo {
        // Store zero or more pieces of text so `echo` stays friendly for multi-word input.
        text: Vec<String>,
    },
    /// Show the dummy shell help message.
    Help,
    /// Close the current shell session.
    #[command(alias = "quit")]
    Exit,
}

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
    println!("{}", "Interactive Dummy shell".bright_cyan());
    // Point the user to the help command for discoverability.
    println!("{}", "Type `help` to see available commands.".bright_yellow());
}

// Handle one tokenized command entered in the dummy shell.
fn handle_command<'a>(_: &'a mut (), tokens: &'a [String]) -> CommandFuture<'a> {
    // Box the async block so it matches the shared `CommandFuture` type alias.
    Box::pin(async move {
        // Parse the shell tokens through Clap so commands and arguments stay typed.
        match parse_command(tokens) {
            Ok(DummyCommand::Help) => {
                // Print the help text for the dummy shell.
                engine::print_shell_help::<DummyShellCli>()?;
            }
            Ok(DummyCommand::Ping) => {
                // Reply with `pong` so users can test that the shell is responsive.
                println!("pong");
            }
            Ok(DummyCommand::Echo { text }) => {
                // Print an empty line when the user entered `echo` without extra text.
                if text.is_empty() {
                    println!();
                } else {
                    // Join all provided argument pieces back into one readable sentence.
                    println!("{}", text.join(" "));
                }
            }
            Ok(DummyCommand::Exit) => {
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

        // Keep the shell open after every non-exit command.
        Ok(ShellAction::Continue)
    })
}

// Convert tokenized dummy-shell input into one typed command.
fn parse_command(tokens: &[String]) -> Result<DummyCommand, clap::Error> {
    // Reuse the shared helper so every shell performs the same Clap parsing steps.
    let cli = engine::parse_shell_command::<DummyShellCli>("dummy", tokens)?;
    // Return only the subcommand because that is all the handler needs.
    Ok(cli.command)
}

#[cfg(test)]
mod tests {
    // Import the dummy parser helper so the tests can validate command shapes.
    use super::{DummyCommand, parse_command};

    #[test]
    fn parses_ping_as_a_valid_command() {
        // Parse the simplest valid dummy-shell command.
        let command = parse_command(&[String::from("ping")]).expect("command should parse");

        // Confirm that Clap maps the input to the ping variant.
        assert!(matches!(command, DummyCommand::Ping));
    }

    #[test]
    fn keeps_quoted_echo_text_together() {
        // Parse an `echo` command whose text should stay in one argument after tokenization.
        let command = parse_command(&[String::from("echo"), String::from("hello world")])
            .expect("command should parse");

        // Confirm that the parsed command keeps the full text as one argument entry.
        assert!(matches!(
            command,
            DummyCommand::Echo { text } if text == vec![String::from("hello world")]
        ));
    }

    #[test]
    fn keeps_multiple_unquoted_echo_arguments() {
        // Parse an `echo` command that contains multiple separate words.
        let command = parse_command(&[
            String::from("echo"),
            String::from("hello"),
            String::from("world"),
        ])
        .expect("command should parse");

        // Confirm that the parsed command stores every word so the handler can join them later.
        assert!(matches!(
            command,
            DummyCommand::Echo { text } if text == vec![String::from("hello"), String::from("world")]
        ));
    }

    #[test]
    fn rejects_unknown_commands() {
        // Parse a command name that the dummy shell does not support.
        let error = parse_command(&[String::from("unknown")]).expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap clearly reports the invalid subcommand.
        assert!(rendered_error.contains("unrecognized subcommand"));
    }
}
