//! magicopilot: the unmodified GitHub Copilot CLI with the Magicodex magic circle above it.
//!
//! Copilot runs in a pseudo console. Its screen is emulated and drawn below a region where
//! the circle grows while the agent works, orbited by the prompt and the agent's public
//! replies, and releases its outlet when the final answer arrives.

mod app;
mod circle;
mod input;
mod launch;
mod magic;
mod pty;
mod render;
mod screen;
mod session;

use launch::Action;

fn main() {
    let options = launch::parse(std::env::args_os().skip(1).collect(), |name| {
        std::env::var_os(name)
    });
    if let Some(error) = &options.error {
        eprintln!("magicopilot: {error}\n\n{}", launch::HELP);
        std::process::exit(2);
    }
    let code = match options.action {
        Action::Help => {
            print!("{}", launch::HELP);
            0
        }
        Action::Version => {
            println!("magicopilot {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Action::PassThrough => app::pass_through(&options),
        Action::Wrap => match app::run(&options) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("magicopilot: {err}");
                1
            }
        },
    };
    std::process::exit(code);
}
