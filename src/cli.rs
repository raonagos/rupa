// re-export Parser
pub use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "Rupā VPN")]
#[command(version, about = "Manages WireGuard VPNs with Rupā")]
pub struct AppCli {
    /// Specify the mode to run the server.
    /// The master mode allows you to connect multiple edge servers and manage them all in one place
    #[arg(short = 'm', long, value_enum, env = "RUPA_MODE", default_value_t = Modes::Master)]
    pub mode: Modes,

    /// Optional external site identifier (e.g., pf, fr, us, cn)
    #[arg(long, env = "RUPA_SITE_EXT")]
    pub site_ext: Option<String>,

    /// Path to the Rupā configuration directory
    #[arg(long, env = "RUPA_PATH", default_value = "/etc/rupa")]
    pub config_path: PathBuf,

    /// Path to the Rupā database directory
    #[arg(long, env = "RUPA_DATABASE_PATH", default_value = "/var/lib/rupa")]
    pub database_path: PathBuf,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Modes {
    /// Run the server as the main server (manages multiple edge servers)
    Master,
    /// Run the server as an edge server (managed by a master server)
    Edge,
}
