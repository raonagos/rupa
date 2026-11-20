use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use rupa::dns::{DnsServer, DnsTable, SharedTable, DEFAULT_TTL};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

async fn start_server(table: DnsTable) -> SocketAddr {
    let shared: SharedTable = Arc::new(RwLock::new(table));
    let server = DnsServer::bind(shared, &["127.0.0.1:0".parse().unwrap()])
        .await
        .expect("bind DNS server");
    let address = server.local_addrs()[0];
    tokio::spawn(server.run());
    address
}

fn build_query(name: &str, query_type: RecordType) -> Vec<u8> {
    let mut message = Message::new(0x1234, MessageType::Query, OpCode::Query);
    message.metadata.recursion_desired = true;
    message.queries.push(Query::query(
        Name::from_ascii(name).expect("query name"),
        query_type,
    ));
    message.to_vec().expect("encode query")
}

async fn udp_exchange(address: SocketAddr, name: &str, query_type: RecordType) -> Message {
    let query = build_query(name, query_type);
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.send_to(&query, address).await.unwrap();
    let mut buffer = [0u8; 4096];
    let (len, _) = timeout(Duration::from_secs(2), socket.recv_from(&mut buffer))
        .await
        .expect("DNS response timeout")
        .expect("DNS response");
    Message::from_vec(&buffer[..len]).expect("decode response")
}

async fn tcp_exchange(address: SocketAddr, name: &str, query_type: RecordType) -> Message {
    let query = build_query(name, query_type);
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(&(query.len() as u16).to_be_bytes())
        .await
        .unwrap();
    stream.write_all(&query).await.unwrap();
    let mut length = [0u8; 2];
    stream.read_exact(&mut length).await.unwrap();
    let len = u16::from_be_bytes(length) as usize;
    let mut buffer = vec![0u8; len];
    stream.read_exact(&mut buffer).await.unwrap();
    Message::from_vec(&buffer).expect("decode response")
}

fn table_with_records() -> DnsTable {
    let mut table = DnsTable::new(Some("internal.example.com".into()), DEFAULT_TTL, Vec::new());
    table.insert_record("site1.internal.example.com", "10.8.0.1".parse().unwrap());
    table.insert_record("nas.internal.example.com", "10.8.0.2".parse().unwrap());
    table
}

#[tokio::test]
async fn answers_a_and_ptr_records() {
    let address = start_server(table_with_records()).await;

    let response = udp_exchange(address, "site1.internal.example.com.", RecordType::A).await;
    assert_eq!(response.metadata.response_code, ResponseCode::NoError);
    assert_eq!(response.answers.len(), 1);
    match &response.answers[0].data {
        RData::A(a) => assert_eq!(a.0, Ipv4Addr::new(10, 8, 0, 1)),
        other => panic!("unexpected record {other:?}"),
    }

    let response = udp_exchange(address, "2.0.8.10.in-addr.arpa.", RecordType::PTR).await;
    assert_eq!(response.answers.len(), 1);
    match &response.answers[0].data {
        RData::PTR(ptr) => assert_eq!(
            ptr.0.to_utf8().trim_end_matches('.'),
            "nas.internal.example.com"
        ),
        other => panic!("unexpected record {other:?}"),
    }
}

#[tokio::test]
async fn returns_nxdomain_and_nodata_with_soa() {
    let address = start_server(table_with_records()).await;

    let response = udp_exchange(address, "missing.internal.example.com.", RecordType::A).await;
    assert_eq!(response.metadata.response_code, ResponseCode::NXDomain);
    assert_eq!(response.authorities.len(), 1);
    assert_eq!(response.authorities[0].record_type(), RecordType::SOA);

    let response = udp_exchange(address, "site1.internal.example.com.", RecordType::AAAA).await;
    assert_eq!(response.metadata.response_code, ResponseCode::NoError);
    assert!(response.answers.is_empty());
    assert_eq!(response.authorities[0].record_type(), RecordType::SOA);
}

#[tokio::test]
async fn answers_over_tcp() {
    let address = start_server(table_with_records()).await;
    let response = tcp_exchange(address, "nas.internal.example.com.", RecordType::A).await;
    assert_eq!(response.answers.len(), 1);
    match &response.answers[0].data {
        RData::A(a) => assert_eq!(a.0, Ipv4Addr::new(10, 8, 0, 2)),
        other => panic!("unexpected record {other:?}"),
    }
}

#[tokio::test]
async fn forwards_unknown_names_to_the_upstream() {
    let upstream = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        loop {
            let (len, peer) = upstream.recv_from(&mut buffer).await.unwrap();
            let request = Message::from_vec(&buffer[..len]).unwrap();
            let query = request.queries[0].clone();
            let mut response = Message::response(request.metadata.id, request.metadata.op_code);
            response.metadata.recursion_available = true;
            response.queries.push(query.clone());
            response.answers.push(Record::from_rdata(
                query.name().clone(),
                60,
                RData::A(A(Ipv4Addr::new(203, 0, 113, 5))),
            ));
            upstream
                .send_to(&response.to_vec().unwrap(), peer)
                .await
                .unwrap();
        }
    });

    let mut table = DnsTable::new(Some("internal.example.com".into()), DEFAULT_TTL, Vec::new());
    table.upstreams = vec![SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        upstream_address.port(),
    )];
    let address = start_server(table).await;

    let response = udp_exchange(address, "example.com.", RecordType::A).await;
    assert_eq!(response.answers.len(), 1);
    match &response.answers[0].data {
        RData::A(a) => assert_eq!(a.0, Ipv4Addr::new(203, 0, 113, 5)),
        other => panic!("unexpected record {other:?}"),
    }
}

#[tokio::test]
async fn answers_aaaa_with_the_server_ipv6_address() {
    let mut table = table_with_records();
    let server_v6: Ipv6Addr = "fd12:3456:789a::1".parse().unwrap();
    table.set_server_v6(Some(server_v6));
    let address = start_server(table).await;

    let response = udp_exchange(address, "site1.internal.example.com.", RecordType::AAAA).await;
    assert_eq!(response.metadata.response_code, ResponseCode::NoError);
    assert_eq!(response.answers.len(), 1);
    match &response.answers[0].data {
        RData::AAAA(record) => assert_eq!(record.0, server_v6),
        other => panic!("unexpected record {other:?}"),
    }
}
