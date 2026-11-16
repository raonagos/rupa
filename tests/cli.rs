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
