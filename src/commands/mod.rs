pub mod dns;
pub mod doctor;
pub mod init;
pub mod peer;
pub mod status;

use anyhow::{bail, Result};
use serde_json::Value;

use crate::cli::{Cli, Command};
use crate::runtime;

pub fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => init::run(&cli.db, args),
        Command::Peer(command) => peer::run(&cli.db, command),
        Command::Dns(command) => dns::run(&cli.db, command),
        Command::Status { json } => status::run(&cli.db, json),
        Command::Doctor => doctor::run(&cli.db),
        Command::Up => runtime::run_blocking(&cli.db),
        Command::Down => {
            let response = runtime::request_down()?;
            ensure_ok(&response)?;
            println!("rupa stopped");
            Ok(())
        }
        Command::Reload => {
            let response = runtime::request_reload()?;
            ensure_ok(&response)?;
            println!(
                "Reloaded {} peers and {} DNS records",
                response["peers"].as_u64().unwrap_or(0),
                response["dns_records"].as_u64().unwrap_or(0),
            );
            if let Some(fields) = response["restart_required"].as_array() {
                if !fields.is_empty() {
                    let names: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
                    println!("Restart required for: {}", names.join(", "));
                }
            }
            Ok(())
        }
    }
}

fn ensure_ok(response: &Value) -> Result<()> {
    if response["ok"].as_bool().unwrap_or(false) {
        Ok(())
    } else {
        bail!(
            "rupa server error: {}",
            response["error"].as_str().unwrap_or("unknown error")
        )
    }
}
