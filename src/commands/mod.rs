pub mod dns;
pub mod doctor;
pub mod init;
pub mod peer;
pub mod status;

use anyhow::{bail, Result};

use crate::cli::{Cli, Command};

pub fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => init::run(&cli.db, args),
        Command::Peer(command) => peer::run(&cli.db, command),
        Command::Dns(command) => dns::run(&cli.db, command),
        Command::Status { json } => status::run(&cli.db, json),
        Command::Doctor => doctor::run(&cli.db),
        Command::Up => bail!("`rupa up` is not implemented yet (runtime phase P3)"),
        Command::Down => bail!("`rupa down` is not implemented yet (runtime phase P3)"),
        Command::Reload => bail!("`rupa reload` is not implemented yet (runtime phase P3)"),
    }
}
