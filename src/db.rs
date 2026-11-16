use std::collections::HashSet;
use std::fs;
use std::net::Ipv4Addr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ipnet::Ipv4Net;
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{DnsRecord, DnsSource, Peer, PeerWithDns, Server};

pub const SCHEMA_VERSION: i64 = 1;

pub const SCHEMA_V1: &str = r#"
CREATE TABLE servers (
    id             INTEGER PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    private_key    BLOB NOT NULL CHECK (length(private_key) = 32),
    public_key     BLOB NOT NULL CHECK (length(public_key) = 32),
    listen_port    INTEGER NOT NULL CHECK (listen_port BETWEEN 1 AND 65535),
    address        TEXT NOT NULL,
    subnet         TEXT NOT NULL,
    mtu            INTEGER NOT NULL DEFAULT 1420,
    endpoint       TEXT,
    interface      TEXT NOT NULL DEFAULT 'rupa0',
    dns_zone       TEXT,
    dns_upstreams  TEXT NOT NULL DEFAULT '1.1.1.1,9.9.9.9',
    dns_listen     TEXT,
    created_at     TEXT NOT NULL
);

CREATE TABLE peers (
    id           INTEGER PRIMARY KEY,
    server_id    INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    private_key  BLOB NOT NULL CHECK (length(private_key) = 32),
    public_key   BLOB NOT NULL CHECK (length(public_key) = 32),
    address      TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    UNIQUE (server_id, name),
    UNIQUE (server_id, public_key),
    UNIQUE (server_id, address)
);

CREATE TABLE dns_records (
    id          INTEGER PRIMARY KEY,
    server_id   INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    address     TEXT NOT NULL,
    source      TEXT NOT NULL DEFAULT 'manual'
                CHECK (source IN ('manual', 'uping', 'peer')),
    peer_id     INTEGER REFERENCES peers(id) ON DELETE CASCADE,
    updated_at  TEXT NOT NULL,
    UNIQUE (server_id, name)
);

CREATE UNIQUE INDEX dns_records_peer_id
    ON dns_records (peer_id) WHERE peer_id IS NOT NULL;
CREATE INDEX dns_records_address ON dns_records (server_id, address);
"#;

pub struct NewServer {
    pub name: String,
    pub private_key: [u8; 32],
    pub public_key: [u8; 32],
    pub listen_port: u16,
    pub address: Ipv4Addr,
    pub subnet: Ipv4Net,
    pub mtu: u16,
    pub endpoint: String,
    pub interface: String,
    pub dns_zone: Option<String>,
    pub dns_upstreams: String,
    pub dns_listen: Option<String>,
    pub created_at: String,
}

/// Create a new database. Refuses to touch an existing file.
pub fn create(path: &Path) -> Result<Connection> {
    if path.exists() {
        bail!("database already exists: {}", path.display());
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let existed = parent.exists();
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
            if !existed {
                set_mode(parent, 0o700)?;
            }
        }
    }

    let mut conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    configure(&conn)?;
    migrate(&mut conn)?;
    set_mode(path, 0o600)?;
    Ok(conn)
}

/// Open an initialized database.
pub fn open(path: &Path) -> Result<Connection> {
    if !path.exists() {
        bail!("database not found: {} (run `rupa init`)", path.display());
    }
    let conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    configure(&conn)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 0 {
        bail!(
            "database {} is not initialized (run `rupa init`)",
            path.display()
        );
    }
    if version != SCHEMA_VERSION {
        bail!(
            "database {} has schema v{version}, this rupa expects v{SCHEMA_VERSION}",
            path.display()
        );
    }
    Ok(conn)
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0 => {
            let tx = conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        v if v == SCHEMA_VERSION => Ok(()),
        v => bail!("database schema v{v} is not supported (expected v{SCHEMA_VERSION})"),
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;",
    )?;
    // WAL is the right mode for the file database; ignore failures on
    // read-only or special filesystems.
    let _ = conn.execute_batch("PRAGMA journal_mode = WAL;");
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    let mut perms = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
        .with_context(|| format!("failed to chmod {}", path.display()))?;
    Ok(())
}

pub fn insert_server(conn: &Connection, server: &NewServer) -> Result<i64> {
    conn.execute(
        "INSERT INTO servers
           (name, private_key, public_key, listen_port, address, subnet, mtu,
            endpoint, interface, dns_zone, dns_upstreams, dns_listen, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            &server.name,
            server.private_key.as_slice(),
            server.public_key.as_slice(),
            i64::from(server.listen_port),
            server.address.to_string(),
            server.subnet.to_string(),
            i64::from(server.mtu),
            &server.endpoint,
            &server.interface,
            server.dns_zone.as_deref(),
            &server.dns_upstreams,
            server.dns_listen.as_deref(),
            &server.created_at,
        ],
    )
    .context("failed to insert the server row")?;
    Ok(conn.last_insert_rowid())
}

pub fn load_server(conn: &Connection) -> Result<Server> {
    let raw = conn
        .query_row(
            "SELECT id, name, private_key, public_key, listen_port, address,
                    subnet, mtu, endpoint, interface, dns_zone, dns_upstreams,
                    dns_listen, created_at
             FROM servers ORDER BY id LIMIT 1",
            [],
            |row| {
                Ok(RawServer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    private_key: row.get(2)?,
                    public_key: row.get(3)?,
                    listen_port: row.get(4)?,
                    address: row.get(5)?,
                    subnet: row.get(6)?,
                    mtu: row.get(7)?,
                    endpoint: row.get(8)?,
                    interface: row.get(9)?,
                    dns_zone: row.get(10)?,
                    dns_upstreams: row.get(11)?,
                    dns_listen: row.get(12)?,
                    created_at: row.get(13)?,
                })
            },
        )
        .optional()?
        .context("no server configured; run `rupa init`")?;
    raw.into_server()
}

pub fn used_addresses(conn: &Connection, server_id: i64) -> Result<HashSet<Ipv4Addr>> {
    let mut stmt = conn.prepare("SELECT address FROM peers WHERE server_id = ?1")?;
    let rows = stmt.query_map([server_id], |row| row.get::<_, String>(0))?;
    let mut used = HashSet::new();
    for row in rows {
        let value = row?;
        if let Ok(address) = value.parse::<Ipv4Addr>() {
            used.insert(address);
        }
    }
    Ok(used)
}

pub fn peer_exists(conn: &Connection, server_id: i64, name: &str) -> Result<bool> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM peers WHERE server_id = ?1 AND name = ?2",
            params![server_id, name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(id.is_some())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_peer(
    conn: &Connection,
    server_id: i64,
    name: &str,
    private_key: &[u8; 32],
    public_key: &[u8; 32],
    address: Ipv4Addr,
    created_at: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO peers
           (server_id, name, private_key, public_key, address, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            server_id,
            name,
            private_key.as_slice(),
            public_key.as_slice(),
            address.to_string(),
            created_at,
        ],
    )
    .context("failed to insert the peer")?;
    Ok(conn.last_insert_rowid())
}

pub fn find_peer(conn: &Connection, server_id: i64, name: &str) -> Result<Option<Peer>> {
    let raw = conn
        .query_row(
            "SELECT id, server_id, name, private_key, public_key, address,
                    created_at, enabled
             FROM peers WHERE server_id = ?1 AND name = ?2",
            params![server_id, name],
            |row| {
                Ok(RawPeer {
                    id: row.get(0)?,
                    server_id: row.get(1)?,
                    name: row.get(2)?,
                    private_key: row.get(3)?,
                    public_key: row.get(4)?,
                    address: row.get(5)?,
                    created_at: row.get(6)?,
                    enabled: row.get(7)?,
                })
            },
        )
        .optional()?;
    raw.map(RawPeer::into_peer).transpose()
}

pub fn list_peers(conn: &Connection, server_id: i64) -> Result<Vec<PeerWithDns>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.server_id, p.name, p.private_key, p.public_key,
                p.address, p.created_at, p.enabled, d.name
         FROM peers p
         LEFT JOIN dns_records d ON d.peer_id = p.id
         WHERE p.server_id = ?1",
    )?;
    let rows = stmt.query_map([server_id], |row| {
        Ok(RawPeerWithDns {
            peer: RawPeer {
                id: row.get(0)?,
                server_id: row.get(1)?,
                name: row.get(2)?,
                private_key: row.get(3)?,
                public_key: row.get(4)?,
                address: row.get(5)?,
                created_at: row.get(6)?,
                enabled: row.get(7)?,
            },
            dns_name: row.get(8)?,
        })
    })?;

    let mut peers = Vec::new();
    for row in rows {
        let raw = row?;
        peers.push(PeerWithDns {
            peer: raw.peer.into_peer()?,
            dns_name: raw.dns_name,
        });
    }
    peers.sort_by_key(|entry| u32::from(entry.peer.address));
    Ok(peers)
}

pub fn delete_peer(conn: &Connection, server_id: i64, name: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM peers WHERE server_id = ?1 AND name = ?2",
        params![server_id, name],
    )?;
    Ok(changed > 0)
}

pub fn find_dns_record(conn: &Connection, server_id: i64, name: &str) -> Result<Option<DnsRecord>> {
    let raw = conn
        .query_row(
            "SELECT id, server_id, name, address, source, peer_id, updated_at
             FROM dns_records WHERE server_id = ?1 AND name = ?2",
            params![server_id, name],
            |row| {
                Ok(RawDns {
                    id: row.get(0)?,
                    server_id: row.get(1)?,
                    name: row.get(2)?,
                    address: row.get(3)?,
                    source: row.get(4)?,
                    peer_id: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .optional()?;
    raw.map(RawDns::into_record).transpose()
}

pub fn list_dns_records(conn: &Connection, server_id: i64) -> Result<Vec<DnsRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, server_id, name, address, source, peer_id, updated_at
         FROM dns_records WHERE server_id = ?1 ORDER BY name",
    )?;
    let rows = stmt.query_map([server_id], |row| {
        Ok(RawDns {
            id: row.get(0)?,
            server_id: row.get(1)?,
            name: row.get(2)?,
            address: row.get(3)?,
            source: row.get(4)?,
            peer_id: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row?.into_record()?);
    }
    Ok(records)
}

pub fn upsert_dns_record(
    conn: &Connection,
    server_id: i64,
    name: &str,
    address: Ipv4Addr,
    source: DnsSource,
    peer_id: Option<i64>,
    updated_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO dns_records
           (server_id, name, address, source, peer_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(server_id, name) DO UPDATE SET
           address = excluded.address,
           source = excluded.source,
           peer_id = excluded.peer_id,
           updated_at = excluded.updated_at",
        params![
            server_id,
            name,
            address.to_string(),
            source.as_str(),
            peer_id,
            updated_at,
        ],
    )
    .context("failed to write the DNS record")?;
    Ok(())
}

pub fn delete_dns_record(conn: &Connection, server_id: i64, name: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM dns_records WHERE server_id = ?1 AND name = ?2",
        params![server_id, name],
    )?;
    Ok(changed > 0)
}

struct RawServer {
    id: i64,
    name: String,
    private_key: Vec<u8>,
    public_key: Vec<u8>,
    listen_port: i64,
    address: String,
    subnet: String,
    mtu: i64,
    endpoint: Option<String>,
    interface: String,
    dns_zone: Option<String>,
    dns_upstreams: String,
    dns_listen: Option<String>,
    created_at: String,
}

impl RawServer {
    fn into_server(self) -> Result<Server> {
        let address: Ipv4Addr = self
            .address
            .parse()
            .with_context(|| format!("invalid server address '{}'", self.address))?;
        let subnet: Ipv4Net = self
            .subnet
            .parse()
            .with_context(|| format!("invalid server subnet '{}'", self.subnet))?;
        if !(1..=65535).contains(&self.listen_port) {
            bail!("invalid listen port {}", self.listen_port);
        }
        if !(0..=65535).contains(&self.mtu) {
            bail!("invalid MTU {}", self.mtu);
        }
        Ok(Server {
            id: self.id,
            name: self.name,
            private_key: self.private_key,
            public_key: self.public_key,
            listen_port: self.listen_port as u16,
            address,
            subnet,
            mtu: self.mtu as u16,
            endpoint: self.endpoint,
            interface: self.interface,
            dns_zone: self.dns_zone,
            dns_upstreams: self.dns_upstreams,
            dns_listen: self.dns_listen,
            created_at: self.created_at,
        })
    }
}

struct RawPeer {
    id: i64,
    server_id: i64,
    name: String,
    private_key: Vec<u8>,
    public_key: Vec<u8>,
    address: String,
    created_at: String,
    enabled: i64,
}

impl RawPeer {
    fn into_peer(self) -> Result<Peer> {
        let address: Ipv4Addr = self
            .address
            .parse()
            .with_context(|| format!("invalid peer address '{}'", self.address))?;
        Ok(Peer {
            id: self.id,
            server_id: self.server_id,
            name: self.name,
            private_key: self.private_key,
            public_key: self.public_key,
            address,
            created_at: self.created_at,
            enabled: self.enabled != 0,
        })
    }
}

struct RawPeerWithDns {
    peer: RawPeer,
    dns_name: Option<String>,
}

struct RawDns {
    id: i64,
    server_id: i64,
    name: String,
    address: String,
    source: String,
    peer_id: Option<i64>,
    updated_at: String,
}

impl RawDns {
    fn into_record(self) -> Result<DnsRecord> {
        let address: Ipv4Addr = self
            .address
            .parse()
            .with_context(|| format!("invalid DNS record address '{}'", self.address))?;
        Ok(DnsRecord {
            id: self.id,
            server_id: self.server_id,
            name: self.name,
            address,
            source: DnsSource::parse(&self.source)?,
            peer_id: self.peer_id,
            updated_at: self.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;
    use crate::util::now_rfc3339;
    use tempfile::tempdir;

    fn server() -> NewServer {
        let private = [1u8; 32];
        let public = keys::public_from_private(private);
        NewServer {
            name: "test".into(),
            private_key: private,
            public_key: public,
            listen_port: 51820,
            address: "10.8.0.1".parse().unwrap(),
            subnet: "10.8.0.0/24".parse().unwrap(),
            mtu: 1420,
            endpoint: "vpn.example.com".into(),
            interface: "rupa0".into(),
            dns_zone: Some("internal.example.com".into()),
            dns_upstreams: "1.1.1.1".into(),
            dns_listen: None,
            created_at: now_rfc3339(),
        }
    }

    #[test]
    fn create_insert_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rupa.db");
        let mut conn = create(&path).unwrap();
        migrate(&mut conn).unwrap();
        let id = insert_server(&conn, &server()).unwrap();
        assert_eq!(id, 1);

        let loaded = load_server(&conn).unwrap();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.address, "10.8.0.1".parse::<Ipv4Addr>().unwrap());
        assert_eq!(loaded.subnet.to_string(), "10.8.0.0/24");
        assert_eq!(loaded.dns_listen_addrs(), vec!["10.8.0.1:53"]);
    }

    #[test]
    fn peers_and_dns_are_linked() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rupa.db");
        let conn = create(&path).unwrap();
        let server_id = insert_server(&conn, &server()).unwrap();
        let peer_id = insert_peer(
            &conn,
            server_id,
            "alice",
            &[2u8; 32],
            &[3u8; 32],
            "10.8.0.2".parse().unwrap(),
            &now_rfc3339(),
        )
        .unwrap();
        upsert_dns_record(
            &conn,
            server_id,
            "alice.internal.example.com",
            "10.8.0.2".parse().unwrap(),
            DnsSource::Peer,
            Some(peer_id),
            &now_rfc3339(),
        )
        .unwrap();

        let peers = list_peers(&conn, server_id).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].dns_name.as_deref(),
            Some("alice.internal.example.com")
        );

        assert!(delete_peer(&conn, server_id, "alice").unwrap());
        assert!(list_dns_records(&conn, server_id).unwrap().is_empty());
    }
}
