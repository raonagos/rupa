use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, PTR, SOA};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use rusqlite::Connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::timeout;

use crate::db;
use crate::model::Server;

pub mod import;

pub const DEFAULT_TTL: u32 = 60;
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_UDP_MESSAGE: usize = 4096;

pub type SharedTable = Arc<RwLock<DnsTable>>;

/// In-memory view of the records the resolver answers for.
#[derive(Debug, Clone)]
pub struct DnsTable {
    pub zone: Option<String>,
    pub ttl: u32,
    pub upstreams: Vec<SocketAddr>,
    server_v6: Option<Ipv6Addr>,
    records: HashMap<String, Ipv4Addr>,
    ptr: HashMap<Ipv4Addr, String>,
}

impl DnsTable {
    pub fn new(zone: Option<String>, ttl: u32, upstreams: Vec<SocketAddr>) -> Self {
        Self {
            zone: zone.map(|value| normalize_name(&value)),
            ttl,
            upstreams,
            server_v6: None,
            records: HashMap::new(),
            ptr: HashMap::new(),
        }
    }

    pub fn from_db(conn: &Connection, server: &Server) -> Result<Self> {
        let mut table = Self::new(
            server.dns_zone.clone(),
            DEFAULT_TTL,
            parse_upstreams(&server.dns_upstreams),
        );
        table.server_v6 = server.v6_address;
        for record in db::list_dns_records(conn, server.id)? {
            table.insert_record(&record.name, record.address);
        }
        Ok(table)
    }

    /// The address answered for `AAAA` queries in the private zone.
    pub fn set_server_v6(&mut self, address: Option<Ipv6Addr>) {
        self.server_v6 = address;
    }

    pub fn insert_record(&mut self, name: &str, address: Ipv4Addr) {
        let name = normalize_name(name);
        self.records.insert(name.clone(), address);
        self.ptr.entry(address).or_insert(name);
    }

    pub fn lookup_a(&self, name: &str) -> Option<Ipv4Addr> {
        self.records.get(name).copied()
    }

    pub fn lookup_ptr(&self, address: Ipv4Addr) -> Option<&str> {
        self.ptr.get(&address).map(String::as_str)
    }

    pub fn in_zone(&self, name: &str) -> bool {
        match self.zone.as_deref() {
            Some(zone) => name == zone || name.ends_with(&format!(".{zone}")),
            None => false,
        }
    }

    fn resolve(&self, name: &str, qtype: RecordType) -> Resolution {
        let in_zone = self.in_zone(name);
        match qtype {
            RecordType::A | RecordType::ANY => match self.lookup_a(name) {
                Some(address) => Resolution::A(address),
                None if in_zone => {
                    if self.zone.as_deref() == Some(name) {
                        Resolution::NoData
                    } else {
                        Resolution::NxDomain
                    }
                }
                None => Resolution::Forward,
            },
            RecordType::AAAA => {
                if in_zone {
                    match self.server_v6 {
                        Some(address) => Resolution::Aaaa(address),
                        None => Resolution::NoData,
                    }
                } else {
                    Resolution::Forward
                }
            }
            RecordType::PTR => match ipv4_from_ptr(name)
                .and_then(|address| self.lookup_ptr(address).map(ToOwned::to_owned))
            {
                Some(target) => Resolution::Ptr(target),
                None => Resolution::Forward,
            },
            _ => {
                if in_zone {
                    if self.records.contains_key(name) {
                        Resolution::NoData
                    } else {
                        Resolution::NxDomain
                    }
                } else {
                    Resolution::Forward
                }
            }
        }
    }
}

enum Resolution {
    A(Ipv4Addr),
    Aaaa(Ipv6Addr),
    Ptr(String),
    NoData,
    NxDomain,
    Forward,
}

/// A bound DNS server. Call [`DnsServer::run`] to start serving.
pub struct DnsServer {
    udp: Vec<Arc<UdpSocket>>,
    tcp: Vec<TcpListener>,
    table: SharedTable,
}

impl DnsServer {
    pub async fn bind(table: SharedTable, listeners: &[SocketAddr]) -> Result<Self> {
        if listeners.is_empty() {
            bail!("no DNS listen address configured");
        }
        let mut udp = Vec::new();
        let mut tcp = Vec::new();
        for address in listeners {
            let socket = UdpSocket::bind(address)
                .await
                .with_context(|| format!("failed to bind DNS UDP {address}"))?;
            let local = socket
                .local_addr()
                .context("failed to read the DNS UDP address")?;
            let listener = TcpListener::bind(local)
                .await
                .with_context(|| format!("failed to bind DNS TCP {local}"))?;
            udp.push(Arc::new(socket));
            tcp.push(listener);
        }
        Ok(Self { udp, tcp, table })
    }

    pub fn local_addrs(&self) -> Vec<SocketAddr> {
        self.udp
            .iter()
            .filter_map(|socket| socket.local_addr().ok())
            .collect()
    }

    pub async fn run(self) -> Result<()> {
        let mut handles = Vec::new();
        for socket in self.udp {
            handles.push(tokio::spawn(udp_loop(socket, self.table.clone())));
        }
        for listener in self.tcp {
            handles.push(tokio::spawn(tcp_loop(listener, self.table.clone())));
        }
        for handle in handles {
            let _ = handle.await;
        }
        Ok(())
    }
}

pub fn normalize_name(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

fn parse_upstreams(value: &str) -> Vec<SocketAddr> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse::<Ipv4Addr>().ok())
        .map(|ip| SocketAddr::new(IpAddr::V4(ip), 53))
        .collect()
}

fn ipv4_from_ptr(name: &str) -> Option<Ipv4Addr> {
    let prefix = name.strip_suffix(".in-addr.arpa")?;
    let mut labels = prefix.split('.').rev();
    let first: u8 = labels.next()?.parse().ok()?;
    let second: u8 = labels.next()?.parse().ok()?;
    let third: u8 = labels.next()?.parse().ok()?;
    let fourth: u8 = labels.next()?.parse().ok()?;
    if labels.next().is_some() {
        return None;
    }
    Some(Ipv4Addr::new(first, second, third, fourth))
}

async fn udp_loop(socket: Arc<UdpSocket>, table: SharedTable) {
    let mut buffer = vec![0u8; MAX_UDP_MESSAGE];
    loop {
        let (len, peer) = match socket.recv_from(&mut buffer).await {
            Ok(value) => value,
            Err(_) => break,
        };
        let request = buffer[..len].to_vec();
        let socket = socket.clone();
        let table = table.clone();
        tokio::spawn(async move {
            if let Some(response) = handle_request(&table, &request, false).await {
                let _ = socket.send_to(&response, peer).await;
            }
        });
    }
}

async fn tcp_loop(listener: TcpListener, table: SharedTable) {
    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(value) => value,
            Err(_) => break,
        };
        let table = table.clone();
        tokio::spawn(async move {
            let mut length = [0u8; 2];
            if !matches!(
                timeout(UPSTREAM_TIMEOUT, stream.read_exact(&mut length)).await,
                Ok(Ok(_))
            ) {
                return;
            }
            let len = u16::from_be_bytes(length) as usize;
            if len == 0 || len > MAX_UDP_MESSAGE {
                return;
            }
            let mut request = vec![0u8; len];
            if !matches!(
                timeout(UPSTREAM_TIMEOUT, stream.read_exact(&mut request)).await,
                Ok(Ok(_))
            ) {
                return;
            }
            if let Some(response) = handle_request(&table, &request, true).await {
                if response.len() <= u16::MAX as usize {
                    let length = (response.len() as u16).to_be_bytes();
                    if matches!(
                        timeout(UPSTREAM_TIMEOUT, stream.write_all(&length)).await,
                        Ok(Ok(()))
                    ) {
                        let _ = timeout(UPSTREAM_TIMEOUT, stream.write_all(&response)).await;
                    }
                }
            }
        });
    }
}

async fn handle_request(table: &SharedTable, request: &[u8], tcp: bool) -> Option<Vec<u8>> {
    let message = Message::from_vec(request).ok()?;
    if message.metadata.message_type != MessageType::Query {
        return None;
    }
    let query = message.queries.first()?.clone();
    let qname = normalize_name(&query.name().to_utf8());

    let (resolution, zone, ttl, upstreams) = {
        let table = table.read().ok()?;
        (
            table.resolve(&qname, query.query_type()),
            table.zone.clone(),
            table.ttl,
            table.upstreams.clone(),
        )
    };

    match resolution {
        Resolution::Forward => {
            let forwarded = if tcp {
                forward_tcp(request, &upstreams).await
            } else {
                forward_udp(request, &upstreams).await
            };
            Some(forwarded.unwrap_or_else(|| servfail(&message)))
        }
        resolution => {
            let response = build_response(&message, &query, &resolution, zone.as_deref(), ttl);
            response.to_vec().ok()
        }
    }
}

fn build_response(
    message: &Message,
    query: &Query,
    resolution: &Resolution,
    zone: Option<&str>,
    ttl: u32,
) -> Message {
    let mut response = Message::response(message.metadata.id, message.metadata.op_code);
    response.metadata.recursion_desired = message.metadata.recursion_desired;
    response.metadata.recursion_available = true;
    response.queries = message.queries.clone();
    if let Some(edns) = &message.edns {
        response.edns = Some(edns.clone());
    }

    match resolution {
        Resolution::A(address) => {
            response.metadata.authoritative = true;
            response.answers.push(Record::from_rdata(
                query.name().clone(),
                ttl,
                RData::A(A(*address)),
            ));
        }
        Resolution::Aaaa(address) => {
            response.metadata.authoritative = true;
            response.answers.push(Record::from_rdata(
                query.name().clone(),
                ttl,
                RData::AAAA(AAAA(*address)),
            ));
        }
        Resolution::Ptr(target) => {
            response.metadata.authoritative = true;
            if let Ok(name) = Name::from_ascii(format!("{target}.")) {
                response.answers.push(Record::from_rdata(
                    query.name().clone(),
                    ttl,
                    RData::PTR(PTR(name)),
                ));
            }
        }
        Resolution::NoData => {
            response.metadata.authoritative = zone.is_some();
            if let Some(soa) = soa_record(zone, ttl) {
                response.authorities.push(soa);
            }
        }
        Resolution::NxDomain => {
            response.metadata.authoritative = zone.is_some();
            response.metadata.response_code = ResponseCode::NXDomain;
            if let Some(soa) = soa_record(zone, ttl) {
                response.authorities.push(soa);
            }
        }
        Resolution::Forward => {}
    }
    response
}

fn soa_record(zone: Option<&str>, ttl: u32) -> Option<Record> {
    let zone = zone?;
    let owner = Name::from_ascii(format!("{zone}.")).ok()?;
    let mname = Name::from_ascii(format!("ns1.{zone}.")).ok()?;
    let rname = Name::from_ascii(format!("hostmaster.{zone}.")).ok()?;
    let soa = SOA::new(mname, rname, 1, 3600, 600, 86400, ttl);
    Some(Record::from_rdata(owner, ttl, RData::SOA(soa)))
}

fn servfail(message: &Message) -> Vec<u8> {
    let mut response = Message::error_msg(
        message.metadata.id,
        message.metadata.op_code,
        ResponseCode::ServFail,
    );
    response.metadata.recursion_desired = message.metadata.recursion_desired;
    response.queries = message.queries.clone();
    response.to_vec().unwrap_or_default()
}

async fn forward_udp(request: &[u8], upstreams: &[SocketAddr]) -> Option<Vec<u8>> {
    for upstream in upstreams {
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(socket) => socket,
            Err(_) => continue,
        };
        if socket.connect(upstream).await.is_err() {
            continue;
        }
        if socket.send(request).await.is_err() {
            continue;
        }
        let mut buffer = vec![0u8; MAX_UDP_MESSAGE];
        match timeout(UPSTREAM_TIMEOUT, socket.recv(&mut buffer)).await {
            Ok(Ok(len)) if len > 0 => return Some(buffer[..len].to_vec()),
            _ => continue,
        }
    }
    None
}

async fn forward_tcp(request: &[u8], upstreams: &[SocketAddr]) -> Option<Vec<u8>> {
    if request.len() > u16::MAX as usize {
        return None;
    }
    for upstream in upstreams {
        let mut stream = match timeout(UPSTREAM_TIMEOUT, TcpStream::connect(upstream)).await {
            Ok(Ok(stream)) => stream,
            _ => continue,
        };
        let length = (request.len() as u16).to_be_bytes();
        if !matches!(
            timeout(UPSTREAM_TIMEOUT, stream.write_all(&length)).await,
            Ok(Ok(()))
        ) {
            continue;
        }
        if !matches!(
            timeout(UPSTREAM_TIMEOUT, stream.write_all(request)).await,
            Ok(Ok(()))
        ) {
            continue;
        }
        let mut length = [0u8; 2];
        if !matches!(
            timeout(UPSTREAM_TIMEOUT, stream.read_exact(&mut length)).await,
            Ok(Ok(_))
        ) {
            continue;
        }
        let len = u16::from_be_bytes(length) as usize;
        if len == 0 {
            continue;
        }
        let mut buffer = vec![0u8; len];
        match timeout(UPSTREAM_TIMEOUT, stream.read_exact(&mut buffer)).await {
            Ok(Ok(_)) => return Some(buffer),
            _ => continue,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_ptr_names() {
        assert_eq!(
            ipv4_from_ptr("2.0.8.10.in-addr.arpa"),
            Some(Ipv4Addr::new(10, 8, 0, 2))
        );
        assert_eq!(ipv4_from_ptr("2.0.8.10.example.com"), None);
        assert_eq!(ipv4_from_ptr("2.0.8.in-addr.arpa"), None);
        assert_eq!(ipv4_from_ptr("2.0.8.10.9.in-addr.arpa"), None);
    }

    #[test]
    fn zone_membership_is_label_aware() {
        let table = DnsTable::new(Some("internal.example.com".into()), 60, vec![]);
        assert!(table.in_zone("site1.internal.example.com"));
        assert!(table.in_zone("internal.example.com"));
        assert!(!table.in_zone("notinternal.example.com"));
        assert!(!table.in_zone("example.com"));
    }

    #[test]
    fn resolve_answers_nxdomain_and_nodata() {
        let mut table = DnsTable::new(Some("internal.example.com".into()), 60, vec![]);
        table.insert_record("site1.internal.example.com", "10.8.0.1".parse().unwrap());

        assert!(matches!(
            table.resolve("site1.internal.example.com", RecordType::A),
            Resolution::A(_)
        ));
        assert!(matches!(
            table.resolve("missing.internal.example.com", RecordType::A),
            Resolution::NxDomain
        ));
        assert!(matches!(
            table.resolve("site1.internal.example.com", RecordType::AAAA),
            Resolution::NoData
        ));
        assert!(matches!(
            table.resolve("example.com", RecordType::A),
            Resolution::Forward
        ));
    }

    #[test]
    fn answers_aaaa_for_any_name_in_the_private_zone() {
        let mut table = DnsTable::new(Some("internal.example.com".into()), 60, vec![]);
        table.insert_record("site1.internal.example.com", "10.8.0.1".parse().unwrap());
        let server_v6: Ipv6Addr = "fd12:3456:789a::1".parse().unwrap();
        table.set_server_v6(Some(server_v6));

        assert!(matches!(
            table.resolve("site1.internal.example.com", RecordType::AAAA),
            Resolution::Aaaa(address) if address == server_v6
        ));
        assert!(matches!(
            table.resolve("missing.internal.example.com", RecordType::AAAA),
            Resolution::Aaaa(address) if address == server_v6
        ));
        assert!(matches!(
            table.resolve("internal.example.com", RecordType::AAAA),
            Resolution::Aaaa(address) if address == server_v6
        ));
        assert!(matches!(
            table.resolve("example.com", RecordType::AAAA),
            Resolution::Forward
        ));
    }

    #[test]
    fn builds_an_aaaa_response() {
        let query = Query::query(
            Name::from_ascii("site1.internal.example.com.").unwrap(),
            RecordType::AAAA,
        );
        let mut message = Message::new(1, MessageType::Query, hickory_proto::op::OpCode::Query);
        message.queries.push(query.clone());
        let server_v6: Ipv6Addr = "fd12:3456:789a::1".parse().unwrap();

        let response = build_response(
            &message,
            &query,
            &Resolution::Aaaa(server_v6),
            Some("internal.example.com"),
            60,
        );
        assert!(response.metadata.authoritative);
        assert_eq!(response.answers.len(), 1);
        match &response.answers[0].data {
            RData::AAAA(record) => assert_eq!(record.0, server_v6),
            other => panic!("unexpected record {other:?}"),
        }
    }
}
