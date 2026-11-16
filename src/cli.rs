use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

const EXAMPLES: &str = "\
Examples:
  sudo rupa init --endpoint vpn.example.com --dns-zone internal.example.com
  sudo rupa peer add laptop --dns laptop --qr
  sudo rupa peer list
  sudo rupa dns add site1.internal.example.com 10.8.0.1

The database is the configuration: /var/lib/rupa/rupa.db unless --db/RUPA_DB.
";

/// rupa — WireGuard VPN server and private DNS, configured in SQLite.
#[derive(Debug, Parser)]
#[command(name = "rupa", version, about, after_help = EXAMPLES)]
pub struct Cli {
    /// SQLite database path.
    #[arg(
        long,
        global = true,
        env = "RUPA_DB",
        default_value = "/var/lib/rupa/rupa.db"
    )]
    pub db: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create the database, server keypair and settings.
    Init(InitArgs),

    /// Run the WireGuard server and DNS (runtime phase P3).
    Up,

    /// Stop a running server.
    Down,

    /// Show the server configuration and peers.
    Status {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Reload peers and DNS records in the running server.
    Reload,

    /// Check the host and database before starting the server.
    Doctor,

    /// Manage WireGuard clients.
    #[command(subcommand)]
    Peer(PeerCmd),

    /// Manage internal DNS records.
    #[command(subcommand)]
    Dns(DnsCmd),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Server name shown by `rupa status`.
    #[arg(long, default_value = "default")]
    pub name: String,

    /// Public host clients dial (IPv4 address or hostname).
    /// Without it, rupa detects the source address used to reach 1.1.1.1.
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Tunnel subnet, in CIDR form.
    #[arg(long, default_value = "10.8.0.0/24")]
    pub subnet: String,

    /// Server address inside the subnet.
    #[arg(long)]
    pub address: Option<String>,

    /// UDP listen port.
    #[arg(long, default_value_t = 51820)]
    pub port: u16,

    /// TUN interface name.
    #[arg(long, default_value = "rupa0")]
    pub interface: String,

    /// TUN MTU.
    #[arg(long, default_value_t = 1420)]
    pub mtu: u16,

    /// Authoritative private DNS zone (e.g. internal.example.com).
    #[arg(long)]
    pub dns_zone: Option<String>,

    /// Comma-separated upstream resolvers.
    #[arg(long, default_value = "1.1.1.1,9.9.9.9")]
    pub dns_upstreams: String,

    /// Comma-separated DNS listen addresses (IP:port). Default: <address>:53.
    #[arg(long)]
    pub dns_listen: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PeerCmd {
    /// Create a peer and print its wg-quick configuration.
    Add(PeerAddArgs),

    /// List peers.
    List {
        /// Merge live handshake/transfer counters from a running server.
        #[arg(long)]
        live: bool,

        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print the configuration of an existing peer.
    Show {
        /// Peer name.
        name: String,

        /// Render the configuration as a QR code.
        #[arg(long)]
        qr: bool,
    },

    /// Delete a peer and its DNS record.
    Remove {
        /// Peer name.
        name: String,

        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct PeerAddArgs {
    /// Peer name (lowercase letters, digits and hyphens).
    pub name: String,

    /// DNS name: a label (appended to --dns-zone) or a full name.
    #[arg(long)]
    pub dns: Option<String>,

    /// Write the client configuration to this file (mode 0600).
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Render the configuration as a QR code instead of printing it.
    #[arg(long)]
    pub qr: bool,
}

#[derive(Debug, Subcommand)]
pub enum DnsCmd {
    /// Add or replace a manual DNS record.
    Add {
        /// Fully qualified name.
        name: String,

        /// IPv4 address.
        address: String,
    },

    /// List DNS records (manual, imported and peer records).
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },

    /// Delete a manual or imported DNS record.
    Remove {
        /// Fully qualified name.
        name: String,

        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}
