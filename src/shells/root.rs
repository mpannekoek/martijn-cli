use figlet_rs::FIGfont;
use owo_colors::OwoColorize;

use crate::shells::engine::{self, CommandFuture, ShellAction};
use crate::AppResult;

pub(crate) async fn run() -> AppResult<()> {
    engine::run_shell((), |_| print_root_intro(), handle_command).await
}

fn handle_command<'a>(_: &'a mut (), input: &'a str) -> CommandFuture<'a> {
    Box::pin(async move {
        match input {
            "help" => print_help(),
            "/azure" => super::azure::run().await?,
            "/dummy" => super::dummy::run().await?,
            "exit" | "quit" => {
                println!("Closing shell.");
                return Ok(ShellAction::Exit);
            }
            other => println!("Unknown command `{other}`. Type `help` to see available commands."),
        }

        Ok(ShellAction::Continue)
    })
}

fn print_root_intro() {
    print_root_banner();
    println!(
        "{}",
        "Welcome to Martijn CLI. Ready when you are. 🚀"
            .bold()
            .bright_white()
    );
    println!(
        "{}",
        "Launch a shell with `/azure` or `/dummy`, or type `help` to see the available commands."
            .bright_blue()
    );
    println!();
}

fn print_root_banner() {
    let banner = if let Ok(font) = FIGfont::standard() {
        font.convert("MARTIJN CLI").map(|figure| figure.to_string())
    } else {
        None
    };

    match banner {
        Some(text) if !text.trim().is_empty() => {
            for line in text.lines() {
                println!("{}", line.bright_cyan().bold());
            }
        }
        _ => println!("{}", "MARTIJN CLI".bold().bright_cyan()),
    }
}

fn print_help() {
    println!("Available commands:");
    println!("  /azure  Open the Azure shell");
    println!("  /dummy  Open the Dummy shell");
    println!("  help    Show this help message");
    println!("  exit    Close the shell");
}
