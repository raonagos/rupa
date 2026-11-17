use clap::Parser;

use rupa::cli::Cli;
use rupa::commands;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    if let Err(err) = commands::dispatch(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
