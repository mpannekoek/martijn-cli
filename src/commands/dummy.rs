// Import Clap helpers so this module can describe its command model.
use clap::Subcommand;
// Import the shared application result type.
use crate::AppResult;

// List the commands that the non-interactive dummy command group understands.
#[derive(Subcommand, Debug)]
pub(crate) enum DummyCommand {
    /// Reply with `pong`.
    Ping,
    /// Print the provided text.
    Echo {
        // Store zero or more pieces of text so `echo` stays friendly for multi-word input.
        text: Vec<String>,
    },
}

// Run one non-interactive dummy command.
pub(crate) fn run_command(command: DummyCommand) -> AppResult<()> {
    // Match on the parsed command so every branch is visible for learners.
    match command {
        DummyCommand::Ping => {
            // Reply with `pong` so users can test that the command is responsive.
            println!("pong");
        }
        DummyCommand::Echo { text } => {
            // Print an empty line when the user entered `echo` without extra text.
            if text.is_empty() {
                println!();
            } else {
                // Join all provided argument pieces back into one readable sentence.
                println!("{}", text.join(" "));
            }
        }
    }

    // Return success after the selected command has printed its output.
    Ok(())
}

#[cfg(test)]
mod tests {
    // Import Clap's parser trait so tests can parse the dummy command group directly.
    use clap::Parser;

    // Import the dummy command type so the tests can validate command shapes.
    use super::DummyCommand;

    // Describe the test-only parser that wraps the dummy subcommands.
    #[derive(Parser, Debug)]
    #[command(name = "dummy")]
    struct DummyTestCli {
        // Store the parsed dummy subcommand.
        #[command(subcommand)]
        command: DummyCommand,
    }

    // Convert test arguments into a typed dummy command.
    fn parse_command(tokens: &[String]) -> Result<DummyCommand, clap::Error> {
        // Start with a fake binary name because Clap expects argv-style input.
        let arguments = std::iter::once(String::from("dummy")).chain(tokens.iter().cloned());
        // Ask Clap to parse the test command line.
        let cli = DummyTestCli::try_parse_from(arguments)?;
        // Return only the command because that is what the runner receives.
        Ok(cli.command)
    }

    #[test]
    fn parses_ping_as_a_valid_command() {
        // Parse the simplest valid dummy command.
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
        // Parse a command name that the dummy command group does not support.
        let error = parse_command(&[String::from("unknown")]).expect_err("command should fail");
        // Render the Clap error into text so we can inspect the message.
        let rendered_error = error.to_string();

        // Confirm that Clap clearly reports the invalid subcommand.
        assert!(rendered_error.contains("unrecognized subcommand"));
    }
}
