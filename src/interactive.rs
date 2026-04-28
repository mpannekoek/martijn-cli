// Import the FIGlet font loader used to build the banner text.
use figlet_rs::FIGfont;
// Import terminal color helpers so the banner and welcome text stand out.
use owo_colors::OwoColorize;

// Print the small interactive start screen used when no CLI command was provided.
pub(crate) fn print_start_screen() {
    // Show the banner first so the CLI has a clear visual identity.
    print_banner();
    // Print the exact welcome text requested for the temporary interactive mode.
    println!(
        "{}",
        "Welcome to Martijn CLI. Ready when you are. 🚀"
            .bold()
            .bright_white()
    );
    // Point users to the non-interactive command list.
    println!(
        "{}",
        "Run `martijn --help` to see available commands.".bright_yellow()
    );
    // Add a blank line before the tiny future-TUI preview.
    println!();
    // Show a very small menu shape so the future TUI direction is visible.
    println!("{}", "Wat wil je doen?".bright_cyan().bold());
    // Keep the menu items plain and simple because they are only a placeholder.
    println!("1. Azure gerelateerde taken");
    // Keep the second option available for the existing dummy command group.
    println!("2. Dummy gerelateerde taken");
}

// Render the banner, using FIGlet when that succeeds.
fn print_banner() {
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
