use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
#[test]
fn package_release_script_creates_ubuntu_tarball_with_checksum() {
    let project_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = TempDir::new().expect("temp workspace");
    let fake_binary = workspace.path().join("maludb");
    std::fs::write(&fake_binary, "#!/usr/bin/env sh\necho maludb test\n").unwrap();
    let mut permissions = std::fs::metadata(&fake_binary).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_binary, permissions).unwrap();

    let dist_dir = workspace.path().join("dist");
    Command::new("bash")
        .current_dir(&project_root)
        .env("MALUDB_SKIP_BUILD", "1")
        .env("MALUDB_BINARY", &fake_binary)
        .args([
            "scripts/package-release.sh",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--version",
            "0.1.0",
            "--dist-dir",
            dist_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "maludb-0.1.0-x86_64-unknown-linux-gnu.tar.gz",
        ));

    let archive = dist_dir.join("maludb-0.1.0-x86_64-unknown-linux-gnu.tar.gz");
    let checksum = dist_dir.join("maludb-0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256");
    assert!(archive.exists());
    assert!(checksum.exists());

    Command::new("tar")
        .args(["-tzf", archive.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "maludb-0.1.0-x86_64-unknown-linux-gnu/bin/maludb",
        ))
        .stdout(predicate::str::contains(
            "maludb-0.1.0-x86_64-unknown-linux-gnu/README.md",
        ))
        .stdout(predicate::str::contains(
            "maludb-0.1.0-x86_64-unknown-linux-gnu/install.sh",
        ));
}
