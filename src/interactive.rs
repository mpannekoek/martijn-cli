// Import `Display` so helper functions can print any text-like value.
use std::fmt::Display;
// Import the standard output handle so the TUI can draw to the terminal.
use std::io::{self, Write};

// Import cursor movement, keyboard events, and terminal helpers from Crossterm.
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
// Import the FIGlet font loader used to build the banner text.
use figlet_rs::FIGfont;
// Import terminal color helpers so the banner and menu text stand out.
use owo_colors::OwoColorize;

// Import the shared application result type so terminal errors can bubble up cleanly.
use crate::AppResult;

// Store the labels shown in the first menu.
const MAIN_MENU_ITEMS: [&str; 2] = ["Azure", "Dummy"];

// Store the Azure command examples shown in the Azure submenu.
const AZURE_COMMAND_HINTS: [&str; 1] = [
    "Help",
];

// Store the dummy command examples shown in the dummy submenu.
const DUMMY_COMMAND_HINTS: [&str; 1] = [
    "Help",
];

// Describe which screen is currently visible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuScreen {
    // The main screen lets the user choose a command group.
    Main,
    // The Azure screen shows Azure command examples without running them.
    Azure,
    // The dummy screen shows dummy command examples without running them.
    Dummy,
}

// Store the small amount of state the interactive menu needs.
#[derive(Debug, Eq, PartialEq)]
struct MenuState {
    // Remember which screen owns the currently visible menu entries.
    screen: MenuScreen,
    // Remember the selected row by index so arrow keys can move it.
    selected_index: usize,
    // Store whether the event loop should stop after the current key.
    should_exit: bool,
}

impl MenuState {
    // Build the initial state for a fresh TUI session.
    fn new() -> Self {
        // Start on the top-level screen with the first row selected.
        Self {
            // The user should first choose between Azure and dummy tasks.
            screen: MenuScreen::Main,
            // Rust arrays are zero-indexed, so `0` means the first item.
            selected_index: 0,
            // The menu should keep running until a key explicitly exits.
            should_exit: false,
        }
    }

    // Return the entries that belong to the currently active screen.
    fn current_items(&self) -> &'static [&'static str] {
        // Pattern matching makes every screen branch explicit for readers.
        match self.screen {
            // The main menu uses the two top-level task categories.
            MenuScreen::Main => &MAIN_MENU_ITEMS,
            // The Azure submenu uses command examples instead of live actions.
            MenuScreen::Azure => &AZURE_COMMAND_HINTS,
            // The dummy submenu uses command examples instead of live actions.
            MenuScreen::Dummy => &DUMMY_COMMAND_HINTS,
        }
    }

    // Move the selected row one step down, wrapping from the last item to the first.
    fn move_down(&mut self) {
        // Read the current item count once so the arithmetic stays easy to follow.
        let item_count = self.current_items().len();
        // Do nothing for an empty menu, even though our current menus are not empty.
        if item_count == 0 {
            // Returning early keeps the rest of the function free from edge-case checks.
            return;
        }

        // Add one to move down and use modulo to wrap back to zero at the end.
        self.selected_index = (self.selected_index + 1) % item_count;
    }

    // Move the selected row one step up, wrapping from the first item to the last.
    fn move_up(&mut self) {
        // Read the current item count once so the branch below is clear.
        let item_count = self.current_items().len();
        // Do nothing for an empty menu, even though our current menus are not empty.
        if item_count == 0 {
            // Returning early avoids subtracting when there is no valid index.
            return;
        }

        // Check the first row explicitly because subtracting from zero would underflow.
        if self.selected_index == 0 {
            // Wrap to the last valid index when the user moves up from the first row.
            self.selected_index = item_count - 1;
        } else {
            // Subtract one in the normal case to move up by one row.
            self.selected_index -= 1;
        }
    }

    // Activate the selected row with Enter.
    fn activate_selected_item(&mut self) {
        // Only the top-level menu opens submenus in this first TUI version.
        if self.screen != MenuScreen::Main {
            // Submenu entries are hints only, so Enter intentionally does nothing there.
            return;
        }

        // Match the selected top-level row to the submenu it should open.
        match self.selected_index {
            // Row zero is the Azure task category.
            0 => {
                // Switch ownership of the visible entries to the Azure screen.
                self.screen = MenuScreen::Azure;
                // Reset the selection so the submenu starts at its first hint.
                self.selected_index = 0;
            }
            // Row one is the dummy task category.
            1 => {
                // Switch ownership of the visible entries to the dummy screen.
                self.screen = MenuScreen::Dummy;
                // Reset the selection so the submenu starts at its first hint.
                self.selected_index = 0;
            }
            // Extra indices should not exist, but ignoring them keeps the state robust.
            _ => {}
        }
    }

    // Go back one level, or exit when already at the top level.
    fn go_back_or_exit(&mut self) {
        // Pattern matching keeps the navigation rule visible and simple.
        match self.screen {
            // From the main menu, going back means the whole TUI can close.
            MenuScreen::Main => {
                // Mark the state so the event loop stops cleanly.
                self.should_exit = true;
            }
            // From a submenu, going back returns to the main menu.
            MenuScreen::Azure | MenuScreen::Dummy => {
                // Restore the top-level screen.
                self.screen = MenuScreen::Main;
                // Reset to the first top-level item for predictable navigation.
                self.selected_index = 0;
            }
        }
    }

    // Apply one keyboard event to the menu state.
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // Ignore key-release events so one physical key press only changes state once.
        if key_event.kind == KeyEventKind::Release {
            // Returning early keeps the real key handling focused on press-like events.
            return;
        }

        // Match on `KeyCode` so each supported keyboard action is explicit.
        match key_event.code {
            // The up arrow moves the selection to the previous row.
            KeyCode::Up => self.move_up(),
            // The down arrow moves the selection to the next row.
            KeyCode::Down => self.move_down(),
            // Enter activates the selected row.
            KeyCode::Enter => self.activate_selected_item(),
            // Escape goes back from a submenu or exits from the main menu.
            KeyCode::Esc => self.go_back_or_exit(),
            // Lowercase `q` follows the same back/exit rule as Escape.
            KeyCode::Char('q') => self.go_back_or_exit(),
            // Uppercase `Q` is accepted because terminal users often expect that.
            KeyCode::Char('Q') => self.go_back_or_exit(),
            // Any other key is ignored so accidental input does not surprise the user.
            _ => {}
        }
    }
}

// Keep terminal TUI settings enabled for the lifetime of this value.
struct TerminalGuard {
    // Remember whether raw mode was enabled so `Drop` knows what to restore.
    raw_mode_enabled: bool,
    // Remember whether the alternate screen was entered so `Drop` can leave it.
    alternate_screen_entered: bool,
    // Remember whether the cursor was hidden so `Drop` can show it again.
    cursor_hidden: bool,
}

impl TerminalGuard {
    // Enable TUI terminal settings and return a guard that will restore them later.
    fn enable<W>(writer: &mut W) -> io::Result<Self>
    where
        // The writer must implement `Write` so Crossterm can send terminal commands to it.
        W: Write,
    {
        // Start with all cleanup flags set to false.
        let mut guard = Self {
            // Raw mode has not been enabled yet.
            raw_mode_enabled: false,
            // The alternate screen has not been entered yet.
            alternate_screen_entered: false,
            // The cursor has not been hidden yet.
            cursor_hidden: false,
        };

        // Raw mode lets the program receive arrow keys immediately without waiting for Enter.
        terminal::enable_raw_mode()?;
        // Store that raw mode is active so it can be disabled in `Drop`.
        guard.raw_mode_enabled = true;

        // Enter the alternate screen so the TUI does not draw over shell or Cargo output.
        execute!(writer, EnterAlternateScreen)?;
        // Store that the alternate screen is active so it can be left in `Drop`.
        guard.alternate_screen_entered = true;

        // Hide the cursor while the menu owns the terminal screen.
        execute!(writer, Hide)?;
        // Store that the cursor is hidden so it can be shown in `Drop`.
        guard.cursor_hidden = true;

        // Return ownership of the guard to the caller so `Drop` can clean up automatically.
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    // Drop runs automatically when the guard leaves scope, including during early returns.
    fn drop(&mut self) {
        // Create a fresh stdout handle because `Drop` cannot borrow the original one.
        let mut stdout = io::stdout();

        // Show the cursor again if this guard hid it earlier.
        if self.cursor_hidden {
            // Best-effort cleanup is appropriate in `Drop` because it cannot return a `Result`.
            let _ = execute!(stdout, Show);
        }

        // Leave the alternate screen so the user's original terminal buffer comes back.
        if self.alternate_screen_entered {
            // Best-effort cleanup is appropriate in `Drop` because it cannot return a `Result`.
            let _ = execute!(stdout, LeaveAlternateScreen);
        }

        // Disable raw mode last so terminal input/output behavior returns to normal.
        if self.raw_mode_enabled {
            // Best-effort cleanup is appropriate in `Drop` because it cannot return a `Result`.
            let _ = terminal::disable_raw_mode();
        }
    }
}

// Run the keyboard-driven start screen used when no CLI command was provided.
pub(crate) fn run_start_screen() -> AppResult<()> {
    // Create stdout once and borrow it mutably when drawing the screen.
    let mut stdout = io::stdout();
    // Enable raw mode and the alternate screen before drawing the first frame.
    let _terminal_guard = TerminalGuard::enable(&mut stdout)?;
    // Own the menu state in this function so the event loop can mutate it.
    let mut menu_state = MenuState::new();

    // Draw the initial state before waiting for the first key.
    render_start_screen(&mut stdout, &menu_state)?;

    // Keep reading keyboard events until the state asks the loop to stop.
    while !menu_state.should_exit {
        // Read one terminal event; the `?` operator returns errors as `AppResult`.
        let terminal_event = event::read()?;
        // Handle only keyboard events because this TUI does not need mouse or resize input yet.
        if let Event::Key(key_event) = terminal_event {
            // Mutate the pure state based on the pressed key.
            menu_state.handle_key_event(key_event);
            // Stop immediately when the key closed the TUI.
            if menu_state.should_exit {
                // Breaking here avoids one unnecessary redraw during shutdown.
                break;
            }
            // Redraw after every key so the selected row stays visible.
            render_start_screen(&mut stdout, &menu_state)?;
        }
    }

    // Return success after the user exits the interactive menu.
    Ok(())
}

// Redraw the full TUI for the current state.
fn render_start_screen<W>(writer: &mut W, menu_state: &MenuState) -> io::Result<()>
where
    // The writer must implement `Write` so Crossterm and `writeln!` can send bytes to it.
    W: Write,
{
    // Clear the visible terminal and put the cursor in the top-left corner.
    execute!(writer, Clear(ClearType::All), MoveTo(0, 0))?;
    // Show the banner first so the CLI has a clear visual identity.
    write_banner(writer)?;
    // Print the exact welcome text used by the interactive mode.
    write_line(
        writer,
        "Welcome to Martijn CLI. Ready when you are. 🚀"
            .bold()
            .bright_white(),
    )?;
    // Point users to the non-interactive command list.
    write_line(
        writer,
        "Run `martijn --help` to see available commands.".bright_yellow(),
    )?;
    // Add a blank line before the menu content.
    write_blank_line(writer)?;
    // Draw the screen-specific heading and the selectable rows.
    write_menu_body(writer, menu_state)?;
    // Flush stdout so the user sees the redraw immediately.
    writer.flush()?;
    // Report that drawing finished successfully.
    Ok(())
}

// Draw the part of the screen that changes between top-level and submenu views.
fn write_menu_body<W>(writer: &mut W, menu_state: &MenuState) -> io::Result<()>
where
    // The writer must implement `Write` because this helper only writes text.
    W: Write,
{
    // Match the current screen so each heading can be tailored to its menu.
    match menu_state.screen {
        // The main menu asks the user to choose a command group.
        MenuScreen::Main => {
            // Print the top-level prompt.
            write_line(writer, "Wat wil je doen?".bright_cyan().bold())?;
            // Print a compact hint for the supported keys.
            write_line(
                writer,
                "Gebruik ↑/↓, Enter om te kiezen, Esc/q om te stoppen.".bright_black(),
            )?;
        }
        // The Azure submenu only shows command examples in this first version.
        MenuScreen::Azure => {
            // Print the Azure submenu title.
            write_line(writer, "Azure gerelateerde taken".bright_cyan().bold())?;
            // Explain that these rows are hints, not live actions yet.
            write_line(
                writer,
                "Commandovoorbeelden; Enter voert nog niets uit. Esc/q gaat terug.".bright_black(),
            )?;
        }
        // The dummy submenu only shows command examples in this first version.
        MenuScreen::Dummy => {
            // Print the dummy submenu title.
            write_line(writer, "Dummy gerelateerde taken".bright_cyan().bold())?;
            // Explain that these rows are hints, not live actions yet.
            write_line(
                writer,
                "Commandovoorbeelden; Enter voert nog niets uit. Esc/q gaat terug.".bright_black(),
            )?;
        }
    }

    // Add a blank line between the heading text and the rows.
    write_blank_line(writer)?;
    // Draw the rows that belong to the current screen.
    write_selectable_rows(writer, menu_state)?;
    // Report that the menu body was written successfully.
    Ok(())
}

// Draw all rows and mark the selected row with a small arrow.
fn write_selectable_rows<W>(writer: &mut W, menu_state: &MenuState) -> io::Result<()>
where
    // The writer must implement `Write` because each row is printed as text.
    W: Write,
{
    // Borrow the current items instead of copying them, because string slices are static data.
    let items = menu_state.current_items();
    // Iterate with indexes so we can compare each row with the selected index.
    for (index, item) in items.iter().enumerate() {
        // Choose the visible marker for the current row.
        let marker = if index == menu_state.selected_index {
            // A selected row receives the planned `>` marker.
            ">"
        } else {
            // An unselected row keeps the same width so the text stays aligned.
            " "
        };

        // Style the selected row differently so keyboard navigation is easy to see.
        if index == menu_state.selected_index {
            // Print the selected row in bright white.
            write!(
                writer,
                "{} {}\r\n",
                marker.bright_cyan().bold(),
                item.bold()
            )?;
        } else {
            // Print unselected rows in normal terminal color.
            write!(writer, "{} {}\r\n", marker, item)?;
        }
    }

    // Report that all rows were written successfully.
    Ok(())
}

// Render the banner, using FIGlet when that succeeds.
fn write_banner<W>(writer: &mut W) -> io::Result<()>
where
    // The writer must implement `Write` because the banner is printed line by line.
    W: Write,
{
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
                // Write one styled FIGlet line.
                write_line(writer, line.bright_cyan().bold())?;
            }
        }
        _ => {
            // Use a simple fallback banner when FIGlet output is unavailable.
            write_line(writer, "MARTIJN CLI".bold().bright_cyan())?;
        }
    }

    // Report that banner rendering finished successfully.
    Ok(())
}

// Write one terminal line with an explicit carriage return and line feed.
fn write_line<W, T>(writer: &mut W, text: T) -> io::Result<()>
where
    // The writer must implement `Write` because bytes are sent directly to the terminal.
    W: Write,
    // The text must implement `Display` so styled and plain values can use the same helper.
    T: Display,
{
    // Raw mode does not translate `\n` into `\r\n`, so we write both characters ourselves.
    write!(writer, "{}\r\n", text)
}

// Write a blank terminal line with explicit raw-mode-safe line endings.
fn write_blank_line<W>(writer: &mut W) -> io::Result<()>
where
    // The writer must implement `Write` because bytes are sent directly to the terminal.
    W: Write,
{
    // Raw mode does not translate `\n` into `\r\n`, so this blank line uses both characters.
    write!(writer, "\r\n")
}

#[cfg(test)]
mod tests {
    // Import Crossterm key types so tests can exercise the same state API as the real TUI.
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    // Import the menu state and screen enum from the parent module.
    use super::{MenuScreen, MenuState};

    // Build a key event with no modifiers for concise tests.
    fn key(code: KeyCode) -> KeyEvent {
        // Crossterm's constructor fills platform-specific defaults for the key event.
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn moving_down_wraps_from_last_item_to_first_item() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Move from the first item to the second item.
        menu_state.move_down();
        // Move once more so the selection wraps back to the first item.
        menu_state.move_down();

        // Confirm that wrapping returns to the first row.
        assert_eq!(menu_state.selected_index, 0);
    }

    #[test]
    fn moving_up_wraps_from_first_item_to_last_item() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Move up from the first row.
        menu_state.move_up();

        // Confirm that wrapping selects the last top-level row.
        assert_eq!(menu_state.selected_index, 1);
    }

    #[test]
    fn enter_on_top_level_azure_opens_azure_submenu() {
        // Start with the default state where Azure is selected.
        let mut menu_state = MenuState::new();
        // Press Enter through the same key handler used by the real event loop.
        menu_state.handle_key_event(key(KeyCode::Enter));

        // Confirm that Enter moved from the main menu to the Azure submenu.
        assert_eq!(menu_state.screen, MenuScreen::Azure);
        // Confirm that submenu selection starts at the first row.
        assert_eq!(menu_state.selected_index, 0);
    }

    #[test]
    fn enter_on_top_level_dummy_opens_dummy_submenu() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Select the dummy row.
        menu_state.move_down();
        // Press Enter through the same key handler used by the real event loop.
        menu_state.handle_key_event(key(KeyCode::Enter));

        // Confirm that Enter moved from the main menu to the dummy submenu.
        assert_eq!(menu_state.screen, MenuScreen::Dummy);
        // Confirm that submenu selection starts at the first row.
        assert_eq!(menu_state.selected_index, 0);
    }

    #[test]
    fn escape_exits_from_the_top_level_menu() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Press Escape through the same key handler used by the real event loop.
        menu_state.handle_key_event(key(KeyCode::Esc));

        // Confirm that Escape marks the TUI as finished.
        assert!(menu_state.should_exit);
    }

    #[test]
    fn q_exits_from_the_top_level_menu() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Press q through the same key handler used by the real event loop.
        menu_state.handle_key_event(key(KeyCode::Char('q')));

        // Confirm that q marks the TUI as finished.
        assert!(menu_state.should_exit);
    }

    #[test]
    fn escape_returns_from_a_submenu_to_the_top_level_menu() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Open the Azure submenu first.
        menu_state.handle_key_event(key(KeyCode::Enter));
        // Press Escape from inside the submenu.
        menu_state.handle_key_event(key(KeyCode::Esc));

        // Confirm that Escape returned to the main menu instead of exiting.
        assert_eq!(menu_state.screen, MenuScreen::Main);
        // Confirm that returning to the main menu resets the selection.
        assert_eq!(menu_state.selected_index, 0);
        // Confirm that the TUI keeps running after returning from a submenu.
        assert!(!menu_state.should_exit);
    }

    #[test]
    fn q_returns_from_a_submenu_to_the_top_level_menu() {
        // Start with a fresh top-level menu state.
        let mut menu_state = MenuState::new();
        // Open the Azure submenu first.
        menu_state.handle_key_event(key(KeyCode::Enter));
        // Press q from inside the submenu.
        menu_state.handle_key_event(key(KeyCode::Char('q')));

        // Confirm that q returned to the main menu instead of exiting.
        assert_eq!(menu_state.screen, MenuScreen::Main);
        // Confirm that returning to the main menu resets the selection.
        assert_eq!(menu_state.selected_index, 0);
        // Confirm that the TUI keeps running after returning from a submenu.
        assert!(!menu_state.should_exit);
    }
}
