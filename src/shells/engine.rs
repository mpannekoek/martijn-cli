// Import `Future` because command handlers return asynchronous work.
use std::future::Future;
// Import input and output types used to read from stdin and flush the prompt.
use std::io::{self, Write};
// Import `Pin` because boxed futures need a stable memory location.
use std::pin::Pin;

// Import Clap traits so the shell engine can reuse generic parsing and help helpers.
use clap::{CommandFactory, Parser};

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

// Represent one problem found while splitting raw shell input into argument tokens.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TokenizeError {
    // Report that the user started a quote but never closed it.
    UnterminatedQuote,
}

// Turn one raw shell input line into argv-style tokens for Clap parsing.
pub(crate) fn tokenize_input(input: &str) -> Result<Vec<String>, TokenizeError> {
    // Store the finished tokens in the same order as the user typed them.
    let mut tokens: Vec<String> = Vec::new();
    // Build the current token character by character until we hit a separator.
    let mut current_token = String::new();
    // Track whether we are currently inside a quoted section.
    let mut active_quote: Option<char> = None;

    // Inspect every character so we can handle spaces and quotes explicitly.
    for character in input.chars() {
        // Branch on the current quote state because separators behave differently there.
        match active_quote {
            Some(quote_character) => {
                // Close the quote when we see the same quote character again.
                if character == quote_character {
                    active_quote = None;
                } else {
                    // Keep every other character literally while we are inside quotes.
                    current_token.push(character);
                }
            }
            None => {
                // Start a quoted section when we see a single or double quote.
                if character == '"' || character == '\'' {
                    active_quote = Some(character);
                } else if character.is_whitespace() {
                    // Finish the current token when whitespace separates arguments.
                    if !current_token.is_empty() {
                        tokens.push(std::mem::take(&mut current_token));
                    }
                } else {
                    // Keep non-whitespace, non-quote characters as part of the token.
                    current_token.push(character);
                }
            }
        }
    }

    // Return a clear error when the user forgot to close a quote.
    if active_quote.is_some() {
        return Err(TokenizeError::UnterminatedQuote);
    }

    // Push the final token when the line did not end with whitespace.
    if !current_token.is_empty() {
        tokens.push(current_token);
    }

    // Return the argv-like token list for the shell-specific Clap parser.
    Ok(tokens)
}

// Parse one tokenized interactive command into a typed Clap parser value.
pub(crate) fn parse_shell_command<ParserType>(
    command_name: &str,
    tokens: &[String],
) -> Result<ParserType, clap::Error>
where
    // Require Clap's `Parser` trait because it provides `try_parse_from`.
    ParserType: Parser,
{
    // Start with a fake binary name because Clap expects argv-style input.
    let arguments = std::iter::once(String::from(command_name)).chain(tokens.iter().cloned());
    // Ask Clap to parse the interactive command line into the requested typed parser.
    ParserType::try_parse_from(arguments)
}

// Print the generated Clap help output for one interactive shell parser.
pub(crate) fn print_shell_help<ParserType>() -> AppResult<()>
where
    // Require Clap's `CommandFactory` trait because it can build help metadata.
    ParserType: CommandFactory,
{
    // Build a command description from the parser definition.
    let mut command = ParserType::command();
    // Print the generated help output to standard output.
    command.print_long_help()?;
    // Add a newline because Clap's help writer does not always end with one.
    println!();
    // Report that help rendering completed successfully.
    Ok(())
}

// Print one Clap parse error and add a small recovery hint for the REPL user.
pub(crate) fn print_parse_error(error: clap::Error) {
    // Show Clap's human-readable parse error inside the shell.
    print!("{error}");
    // Add one extra hint so recovery is obvious in the REPL.
    println!("Type `help` to see the available commands.");
}

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
    // The handler borrows state and parsed tokens for exactly as long as its future lives.
    Handler: for<'a> FnMut(&'a mut State, &'a [String]) -> CommandFuture<'a>,
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

        // Split the trimmed input into argv-style pieces before shell-specific parsing.
        match tokenize_input(trimmed) {
            Ok(tokens) => {
                // Ask the shell-specific handler what to do with the parsed tokens.
                match handle_command(&mut state, &tokens).await? {
                    ShellAction::Continue => {
                        // Do nothing special here because the loop will print the next prompt.
                    }
                    ShellAction::Exit => {
                        // Stop the loop when the handler explicitly asks to exit.
                        break;
                    }
                }
            }
            Err(TokenizeError::UnterminatedQuote) => {
                // Explain the tokenization problem and point the user to a fix.
                println!(
                    "Your command contains an unterminated quote. Close the quote and try again."
                );
            }
        }
    }

    // Return success once the shell loop ends cleanly.
    Ok(())
}

#[cfg(test)]
mod tests {
    // Import the tokenizer helpers from the parent module so we can verify shell parsing behavior.
    use super::{TokenizeError, tokenize_input};

    #[test]
    fn splits_simple_whitespace_separated_tokens() {
        // Tokenize a regular command with three space-separated parts.
        let tokens = tokenize_input("login tenant-id status").expect("tokenization should succeed");

        // Confirm that whitespace becomes token boundaries.
        assert_eq!(tokens, vec!["login", "tenant-id", "status"]);
    }

    #[test]
    fn keeps_spaces_inside_double_quotes() {
        // Tokenize a command that contains one quoted argument with spaces.
        let tokens = tokenize_input("echo \"hello world\"").expect("tokenization should succeed");

        // Confirm that the quoted text stays together as one argument.
        assert_eq!(tokens, vec!["echo", "hello world"]);
    }

    #[test]
    fn rejects_unterminated_quotes() {
        // Tokenize a command that opens a quote but never closes it.
        let error = tokenize_input("echo \"hello").expect_err("tokenization should fail");

        // Confirm that the tokenizer reports the specific quote error.
        assert_eq!(error, TokenizeError::UnterminatedQuote);
    }
}
