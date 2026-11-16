use clap::Parser;

/// rupa — WireGuard VPN server and private DNS, configured in SQLite.
#[derive(Debug, Parser)]
#[command(name = "rupa", version, about)]
struct Cli {}

fn main() {
    Cli::parse();
    println!(
        "rupa {} — scaffold only, the CLI is not implemented yet",
        env!("CARGO_PKG_VERSION")
    );
}
