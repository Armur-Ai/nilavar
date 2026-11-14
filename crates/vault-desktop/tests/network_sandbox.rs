//! The macOS no-network sandbox denies sockets but not file access.
//! (Runs in its own test process because the sandbox cannot be removed.)

#[cfg(target_os = "macos")]
#[test]
fn no_network_sandbox_denies_sockets_and_keeps_files() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(
        std::net::TcpStream::connect(addr).is_ok(),
        "control: networking works before"
    );
    assert!(vault_desktop::platform::deny_network());
    assert!(
        std::net::TcpStream::connect(addr).is_err(),
        "TCP must be denied"
    );
    assert!(
        std::net::UdpSocket::bind("127.0.0.1:0").is_err(),
        "UDP must be denied"
    );
    assert!(
        std::net::TcpListener::bind("127.0.0.1:0").is_err(),
        "listening must be denied"
    );
    // Vault-style atomic file work still functions.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.nvault.age");
    vault_core::storage::write_atomic(&p, b"ciphertext", None, vault_core::storage::Faults::NONE)
        .unwrap();
    vault_core::storage::write_atomic(
        &p,
        b"ciphertext2",
        Some(vault_core::storage::Fingerprint::of(b"ciphertext")),
        vault_core::storage::Faults::NONE,
    )
    .unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"ciphertext2");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn network_sandbox_reported_unavailable() {
    assert!(!vault_desktop::platform::deny_network());
}
