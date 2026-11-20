use clap::Parser;

use rupa::cli::Cli;
use rupa::commands;

fn main() {
    install_panic_hook();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    if let Err(err) = commands::dispatch(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

/// A panicking worker thread must take the whole process down.
///
/// The userspace WireGuard runtime spawns one handshake worker per CPU. A worker
/// that panics is simply gone: no thread joins it, the process keeps running and
/// `rupa status` keeps answering, but no handshake can ever complete again — the
/// tunnel is dead while systemd still sees a healthy service. (Concretely,
/// wireguard-rs panics with `released id not allocated` when a handshake
/// initiation is retransmitted before the previous response is confirmed.)
///
/// Aborting hands the problem to systemd: `Restart=on-failure` brings the daemon
/// back in a couple of seconds instead of leaving it silently broken for hours.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        eprintln!("rupa: a worker panicked, aborting so systemd can restart the tunnel");
        std::process::abort();
    }));
}
