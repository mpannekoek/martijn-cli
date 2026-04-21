use crate::shells::engine::{self, CommandFuture, ShellAction};
use crate::AppResult;

pub(crate) async fn run() -> AppResult<()> {
    engine::run_shell((), |_| print_intro(), handle_command).await
}

fn handle_command<'a>(_: &'a mut (), input: &'a str) -> CommandFuture<'a> {
    Box::pin(async move {
        match input {
            "help" => print_help(),
            "ping" => println!("pong"),
            "exit" | "quit" => {
                println!("Closing shell.");
                return Ok(ShellAction::Exit);
            }
            _ => {
                if let Some(text) = input.strip_prefix("echo ") {
                    println!("{text}");
                } else if input == "echo" {
                    println!();
                } else {
                    println!("Unknown command `{input}`. Type `help` to see available commands.");
                }
            }
        }

        Ok(ShellAction::Continue)
    })
}

fn print_intro() {
    println!("Interactive Dummy shell");
    println!("Type `help` to see available commands.");
}

fn print_help() {
    println!("Available commands:");
    println!("  ping         Print `pong`");
    println!("  echo <text>  Print the provided text");
    println!("  help         Show this help message");
    println!("  exit         Close the shell");
}
