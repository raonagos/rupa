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

    let config = ClientConfig {
        private_key: peer.private_key_b64()?,
        address: format!("{}/{}", peer.address, server.subnet.prefix_len()),
        dns: server.address.to_string(),
        mtu: server.mtu,
        server_public_key: server.public_key_b64()?,
        allowed_ips: server.subnet.to_string(),
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
        assert!(rendered.contains("[Interface]\nPrivateKey = PRIVATE=\n"));
        assert!(rendered.contains("Address = 10.8.0.2/24\n"));
        assert!(rendered.contains("DNS = 10.8.0.1\n"));
        assert!(rendered.contains("[Peer]\nPublicKey = PUBLIC=\n"));
        assert!(rendered.contains("AllowedIPs = 10.8.0.0/24\n"));
        assert!(rendered.contains("Endpoint = vpn.example.com:51820\n"));
        assert!(rendered.ends_with("PersistentKeepalive = 25\n"));
    }
}
