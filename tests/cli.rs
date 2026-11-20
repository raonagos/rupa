use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn rupa(dir: &TempDir) -> Command {
    let mut command = Command::cargo_bin("rupa").expect("rupa binary");
    command.arg("--db").arg(dir.path().join("rupa.db"));
    command
}

fn init(dir: &TempDir) {
    rupa(dir)
        .args([
            "init",
            "--endpoint",
            "vpn.example.com",
            "--dns-zone",
            "internal.example.com",
        ])
        .assert()
        .success();
}

fn init_without_zone(dir: &TempDir) {
    rupa(dir)
        .args(["init", "--endpoint", "vpn.example.com"])
        .assert()
        .success();
}

fn json_output(dir: &TempDir, args: &[&str]) -> Value {
    let output = rupa(dir).args(args).output().expect("run rupa");
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON")
}

#[test]
fn init_creates_a_database_and_status_reports_it() {
    let dir = TempDir::new().unwrap();
    rupa(&dir)
        .args([
            "init",
            "--endpoint",
            "vpn.example.com",
            "--dns-zone",
            "internal.example.com",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"))
        .stdout(predicate::str::contains("10.8.0.1/24"));

    assert!(dir.path().join("rupa.db").exists());

    let status = json_output(&dir, &["status", "--json"]);
    assert_eq!(status["server"]["name"], "default");
    assert_eq!(status["server"]["address"], "10.8.0.1");
    assert_eq!(status["server"]["listen_port"], 51820);
    assert_eq!(status["peers"], 0);
    assert_eq!(status["runtime"], false);
}

#[test]
fn init_refuses_to_overwrite() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir)
        .args(["init", "--endpoint", "vpn.example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn peer_add_renders_a_wg_quick_config() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    rupa(&dir)
        .args(["peer", "add", "alice", "--dns", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[Interface]"))
        .stdout(predicate::str::contains("Address = 10.8.0.2/24"))
        .stdout(predicate::str::contains("DNS = 10.8.0.1"))
        .stdout(predicate::str::contains("[Peer]"))
        .stdout(predicate::str::contains("AllowedIPs = 10.8.0.0/24"))
        .stdout(predicate::str::contains("Endpoint = vpn.example.com:51820"))
        .stdout(predicate::str::contains("PersistentKeepalive = 25"));

    let peers = json_output(&dir, &["peer", "list", "--json"]);
    assert_eq!(peers.as_array().unwrap().len(), 1);
    assert_eq!(peers[0]["name"], "alice");
    assert_eq!(peers[0]["address"], "10.8.0.2");
    assert_eq!(peers[0]["dns"], "alice.internal.example.com");
}

#[test]
fn peers_get_incrementing_addresses() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    rupa(&dir)
        .args(["peer", "add", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Address = 10.8.0.2/24"));
    rupa(&dir)
        .args(["peer", "add", "bob"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Address = 10.8.0.3/24"));

    rupa(&dir)
        .args(["peer", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alice"))
        .stdout(predicate::str::contains("bob"));

    rupa(&dir)
        .args(["peer", "show", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Address = 10.8.0.2/24"));
}

#[test]
fn peer_removal_cascades_to_dns() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir)
        .args(["peer", "add", "alice", "--dns", "alice"])
        .assert()
        .success();

    rupa(&dir)
        .args(["peer", "remove", "alice", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed peer alice"));

    assert!(json_output(&dir, &["peer", "list", "--json"])
        .as_array()
        .unwrap()
        .is_empty());
    assert!(json_output(&dir, &["dns", "list", "--json"])
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn dns_records_can_be_added_updated_and_removed() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    rupa(&dir)
        .args(["dns", "add", "site1.internal.example.com", "10.8.0.1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added DNS record"));
    rupa(&dir)
        .args(["dns", "add", "site1.internal.example.com", "10.8.0.9"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated DNS record"));

    let records = json_output(&dir, &["dns", "list", "--json"]);
    assert_eq!(records.as_array().unwrap().len(), 1);
    assert_eq!(records[0]["address"], "10.8.0.9");
    assert_eq!(records[0]["source"], "manual");

    rupa(&dir)
        .args(["dns", "remove", "site1.internal.example.com", "--yes"])
        .assert()
        .success();
    assert!(json_output(&dir, &["dns", "list", "--json"])
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn peer_dns_records_are_not_removable_from_dns() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir)
        .args(["peer", "add", "alice", "--dns", "alice"])
        .assert()
        .success();

    rupa(&dir)
        .args(["dns", "remove", "alice.internal.example.com", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("managed by a peer"));
}

#[test]
fn rejects_duplicates_and_invalid_names() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir).args(["peer", "add", "alice"]).assert().success();

    rupa(&dir)
        .args(["peer", "add", "alice"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    rupa(&dir)
        .args(["peer", "add", "Bad_Name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("peer name"));

    rupa(&dir)
        .args(["peer", "remove", "missing", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn dns_label_requires_a_zone() {
    let dir = TempDir::new().unwrap();
    init_without_zone(&dir);
    rupa(&dir)
        .args(["peer", "add", "alice", "--dns", "alice"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--dns-zone"));
}

#[test]
fn dns_names_must_be_fully_qualified() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir)
        .args(["dns", "add", "nas", "10.8.0.1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fully qualified"));
}

#[test]
fn peer_output_file_is_written() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    let path = dir.path().join("alice.conf");
    rupa(&dir)
        .args(["peer", "add", "alice", "--output", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Wrote"));
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("[Interface]"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn imports_a_hosts_file_and_prunes() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    let hosts = dir.path().join("uping.hosts");
    std::fs::write(
        &hosts,
        "# generated by uping\n10.8.0.1 site1.internal.example.com\n10.8.0.2 nas.internal.example.com\n",
    )
    .unwrap();

    rupa(&dir)
        .args(["dns", "import", hosts.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported 2 records"));

    let records = json_output(&dir, &["dns", "list", "--json"]);
    assert_eq!(records.as_array().unwrap().len(), 2);
    assert_eq!(records[0]["source"], "uping");

    std::fs::write(&hosts, "10.8.0.1 site1.internal.example.com\n").unwrap();
    rupa(&dir)
        .args(["dns", "import", hosts.to_str().unwrap(), "--prune"])
        .assert()
        .success();
    let records = json_output(&dir, &["dns", "list", "--json"]);
    assert_eq!(records.as_array().unwrap().len(), 1);
    assert_eq!(records[0]["name"], "site1.internal.example.com");
}

#[test]
fn imports_from_stdin() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir)
        .args(["dns", "import", "-"])
        .write_stdin("10.8.0.7 stdin.internal.example.com\n")
        .assert()
        .success();
    let records = json_output(&dir, &["dns", "list", "--json"]);
    assert_eq!(records[0]["address"], "10.8.0.7");
    assert_eq!(records[0]["source"], "uping");
}

#[test]
fn import_refuses_peer_managed_names() {
    let dir = TempDir::new().unwrap();
    init(&dir);
    rupa(&dir)
        .args(["peer", "add", "alice", "--dns", "alice"])
        .assert()
        .success();
    let hosts = dir.path().join("conflict.hosts");
    std::fs::write(&hosts, "10.8.0.1 alice.internal.example.com\n").unwrap();
    rupa(&dir)
        .args(["dns", "import", hosts.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("managed by a peer"));
}

#[test]
fn init_creates_a_random_ipv6_ula_by_default() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    let status = json_output(&dir, &["status", "--json"]);
    let address = status["server"]["v6_address"].as_str().expect("v6 address");
    assert!(address.starts_with("fd"), "unexpected ULA {address}");
    let subnet = status["server"]["v6_subnet"].as_str().expect("v6 subnet");
    assert!(subnet.starts_with("fd"));
    assert!(subnet.ends_with("/64"));
    assert_eq!(status["server"]["tunnel"], "split");

    rupa(&dir)
        .args(["peer", "add", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(", {address}")))
        .stdout(predicate::str::contains("::2/64"));
}

#[test]
fn no_ipv6_keeps_the_v4_only_split_config() {
    let dir = TempDir::new().unwrap();
    rupa(&dir)
        .args(["init", "--endpoint", "vpn.example.com", "--no-ipv6"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ipv6:       disabled"))
        .stdout(predicate::str::contains("tunnel:     split"));

    rupa(&dir)
        .args(["peer", "add", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Address = 10.8.0.2/24\n"))
        .stdout(predicate::str::contains("DNS = 10.8.0.1\n"))
        .stdout(predicate::str::contains("AllowedIPs = 10.8.0.0/24\n"));

    let status = json_output(&dir, &["status", "--json"]);
    assert!(status["server"]["v6_address"].is_null());
    assert!(status["server"]["v6_subnet"].is_null());
    assert_eq!(status["server"]["tunnel"], "split");
}

#[test]
fn explicit_ipv6_subnet_assigns_peer_addresses() {
    let dir = TempDir::new().unwrap();
    rupa(&dir)
        .args([
            "init",
            "--endpoint",
            "vpn.example.com",
            "--v6-subnet",
            "fd12:3456:789a::/64",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ipv6:       fd12:3456:789a::1/64"))
        .stdout(predicate::str::contains("subnet fd12:3456:789a::/64"));

    rupa(&dir)
        .args(["peer", "add", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Address = 10.8.0.2/24, fd12:3456:789a::2/64",
        ))
        .stdout(predicate::str::contains(
            "DNS = 10.8.0.1, fd12:3456:789a::1",
        ))
        .stdout(predicate::str::contains(
            "AllowedIPs = 10.8.0.0/24, fd12:3456:789a::/64",
        ));

    rupa(&dir)
        .args(["peer", "add", "bob"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Address = 10.8.0.3/24, fd12:3456:789a::3/64",
        ));

    let status = json_output(&dir, &["status", "--json"]);
    assert_eq!(status["server"]["v6_address"], "fd12:3456:789a::1");
    assert_eq!(status["server"]["v6_subnet"], "fd12:3456:789a::/64");
}

#[test]
fn full_tunnel_nats_all_client_traffic() {
    let dir = TempDir::new().unwrap();
    rupa(&dir)
        .args([
            "init",
            "--endpoint",
            "vpn.example.com",
            "--full-tunnel",
            "--v6-subnet",
            "fd12:3456:789a::/64",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("tunnel:     full"))
        .stdout(predicate::str::contains("Full tunnel"));

    rupa(&dir)
        .args(["peer", "add", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Address = 10.8.0.2/24, fd12:3456:789a::2/64",
        ))
        .stdout(predicate::str::contains("AllowedIPs = 0.0.0.0/0, ::/0"));

    let status = json_output(&dir, &["status", "--json"]);
    assert_eq!(status["server"]["tunnel"], "full");
    assert_eq!(status["server"]["full_tunnel"], true);
}

#[test]
fn init_rejects_non_ula_or_non_slash_64_ipv6_subnets() {
    let dir = TempDir::new().unwrap();
    rupa(&dir)
        .args([
            "init",
            "--endpoint",
            "vpn.example.com",
            "--v6-subnet",
            "2001:db8::/64",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("fd00::/8"));

    let dir = TempDir::new().unwrap();
    rupa(&dir)
        .args([
            "init",
            "--endpoint",
            "vpn.example.com",
            "--v6-subnet",
            "fd12:3456:789a::/48",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("/64"));
}
