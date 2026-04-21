use std::future::Future;
use std::io::{self, Write};
use std::pin::Pin;

use crate::AppResult;

pub(crate) enum ShellAction {
    Continue,
    Exit,
}

pub(crate) type CommandFuture<'a> = Pin<Box<dyn Future<Output = AppResult<ShellAction>> + 'a>>;

pub(crate) async fn run_shell<State, Intro, Handler>(
    mut state: State,
    mut print_intro: Intro,
    mut handle_command: Handler,
) -> AppResult<()>
where
    Intro: FnMut(&State),
    Handler: for<'a> FnMut(&'a mut State, &'a str) -> CommandFuture<'a>,
{
    print_intro(&state);

    loop {
        print!("martijn> ");
        io::stdout().flush()?;

        let mut input = String::new();
        let bytes_read = io::stdin().read_line(&mut input)?;
        if bytes_read == 0 {
            println!();
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        match handle_command(&mut state, trimmed).await? {
            ShellAction::Continue => {}
            ShellAction::Exit => break,
        }
    }

    Ok(())
}
