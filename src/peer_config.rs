use anyhow::{Context, Result};

use crate::model::{Peer, Server};

pub struct ClientConfig {
    pub private_key: String,
    pub address: String,
    pub dns: String,
    pub mtu: u16,
    pub server_public_key: String,
    pub allowed_ips: String,
    pub endpoint: String,
    pub keepalive: u16,
}

pub fn build(server: &Server, peer: &Peer) -> Result<String> {
    let endpoint = server
        .endpoint
        .as_deref()
        .context("the server has no endpoint; run `rupa init --endpoint <host>`")?;

    let v4_address = format!("{}/{}", peer.address, server.subnet.prefix_len());
    let address = match peer.v6_address {
        Some(v6_address) => format!("{v4_address}, {v6_address}/64"),
        None => v4_address,
    };
    let dns = match server.v6_address {
        Some(v6_address) => format!("{}, {v6_address}", server.address),
        None => server.address.to_string(),
    };
    let allowed_ips = if server.full_tunnel {
        "0.0.0.0/0, ::/0".to_string()
    } else if let Some(v6_subnet) = server.v6_subnet.filter(|_| server.has_ipv6()) {
        format!("{}, {v6_subnet}", server.subnet)
    } else {
        server.subnet.to_string()
    };

    let config = ClientConfig {
        private_key: peer.private_key_b64()?,
        address,
        dns,
        mtu: server.mtu,
        server_public_key: server.public_key_b64()?,
        allowed_ips,
        endpoint: format!("{endpoint}:{}", server.listen_port),
        keepalive: 25,
    };
    Ok(render(&config))
}

pub fn render(config: &ClientConfig) -> String {
    format!(
        "[Interface]\n\
         PrivateKey = {}\n\
         Address = {}\n\
         DNS = {}\n\
         MTU = {}\n\
         \n\
         [Peer]\n\
         PublicKey = {}\n\
         AllowedIPs = {}\n\
         Endpoint = {}\n\
         PersistentKeepalive = {}\n",
        config.private_key,
        config.address,
        config.dns,
        config.mtu,
        config.server_public_key,
        config.allowed_ips,
        config.endpoint,
        config.keepalive,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(full_tunnel: bool) -> Server {
        Server {
            id: 1,
            name: "test".into(),
            private_key: [1u8; 32].to_vec(),
            public_key: [2u8; 32].to_vec(),
            listen_port: 51820,
            address: "10.8.0.1".parse().unwrap(),
            subnet: "10.8.0.0/24".parse().unwrap(),
            v6_subnet: Some("fd12:3456:789a::/64".parse().unwrap()),
            v6_address: Some("fd12:3456:789a::1".parse().unwrap()),
            full_tunnel,
            mtu: 1420,
            endpoint: Some("vpn.example.com".into()),
            interface: "rupa0".into(),
            dns_zone: Some("internal.example.com".into()),
            dns_upstreams: "1.1.1.1".into(),
            dns_listen: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn peer(v6_address: Option<&str>) -> Peer {
        Peer {
            id: 1,
            server_id: 1,
            name: "alice".into(),
            private_key: [3u8; 32].to_vec(),
            public_key: [4u8; 32].to_vec(),
            address: "10.8.0.2".parse().unwrap(),
            v6_address: v6_address.map(|value| value.parse().unwrap()),
            created_at: "2026-01-01T00:00:00Z".into(),
            enabled: true,
        }
    }

    #[test]
    fn renders_a_standard_wg_quick_config() {
        let config = ClientConfig {
            private_key: "PRIVATE=".into(),
            address: "10.8.0.2/24".into(),
            dns: "10.8.0.1".into(),
            mtu: 1420,
            server_public_key: "PUBLIC=".into(),
            allowed_ips: "10.8.0.0/24".into(),
            endpoint: "vpn.example.com:51820".into(),
            keepalive: 25,
        };
        let rendered = render(&config);
        assert_eq!(
            rendered,
            "[Interface]\n\
             PrivateKey = PRIVATE=\n\
             Address = 10.8.0.2/24\n\
             DNS = 10.8.0.1\n\
             MTU = 1420\n\
             \n\
             [Peer]\n\
             PublicKey = PUBLIC=\n\
             AllowedIPs = 10.8.0.0/24\n\
             Endpoint = vpn.example.com:51820\n\
             PersistentKeepalive = 25\n"
        );
    }

    #[test]
    fn builds_a_v4_only_split_config_unchanged() {
        let mut v4_server = server(false);
        v4_server.v6_subnet = None;
        v4_server.v6_address = None;
        let rendered = build(&v4_server, &peer(None)).unwrap();
        assert!(rendered.contains("Address = 10.8.0.2/24\n"));
        assert!(rendered.contains("DNS = 10.8.0.1\n"));
        assert!(rendered.contains("AllowedIPs = 10.8.0.0/24\n"));
        assert!(!rendered.contains("fd"));
    }

    #[test]
    fn builds_a_v6_split_config() {
        let rendered = build(&server(false), &peer(Some("fd12:3456:789a::2"))).unwrap();
        assert!(rendered.contains("Address = 10.8.0.2/24, fd12:3456:789a::2/64\n"));
        assert!(rendered.contains("DNS = 10.8.0.1, fd12:3456:789a::1\n"));
        assert!(rendered.contains("AllowedIPs = 10.8.0.0/24, fd12:3456:789a::/64\n"));
    }

    #[test]
    fn builds_a_full_tunnel_config() {
        let rendered = build(&server(true), &peer(Some("fd12:3456:789a::2"))).unwrap();
        assert!(rendered.contains("Address = 10.8.0.2/24, fd12:3456:789a::2/64\n"));
        assert!(rendered.contains("DNS = 10.8.0.1, fd12:3456:789a::1\n"));
        assert!(rendered.contains("AllowedIPs = 0.0.0.0/0, ::/0\n"));
        println!("{rendered}");
    }
}
