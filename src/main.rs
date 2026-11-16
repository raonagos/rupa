use clap::Parser;

use rupa::cli::Cli;
use rupa::commands;

fn main() {
    let cli = Cli::parse();
    if let Err(err) = commands::dispatch(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
