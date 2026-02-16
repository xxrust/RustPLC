use std::fs;
use std::process::Command;

#[test]
fn flash_rp2040_copies_uf2_to_mount() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_flash_rp2040_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let uf2 = base.join("firmware.uf2");
    fs::write(&uf2, b"UF2-DUMMY").expect("write uf2");

    let mount = base.join("mnt");
    fs::create_dir_all(&mount).expect("create mount dir");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("flash-rp2040")
        .arg("--uf2")
        .arg(&uf2)
        .arg("--mount")
        .arg(&mount)
        .output()
        .expect("run flash-rp2040");

    assert!(
        output.status.success(),
        "flash-rp2040 should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let copied = mount.join("firmware.uf2");
    assert!(copied.exists());
    assert_eq!(fs::read(&copied).unwrap(), b"UF2-DUMMY");
}

#[test]
fn flash_rp2040_dry_run_does_not_copy() {
    let base = std::env::temp_dir().join(format!(
        "rust_plc_flash_rp2040_dry_run_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");

    let uf2 = base.join("firmware.uf2");
    fs::write(&uf2, b"UF2-DUMMY").expect("write uf2");

    let mount = base.join("mnt");
    fs::create_dir_all(&mount).expect("create mount dir");

    let output = Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .arg("flash-rp2040")
        .arg("--uf2")
        .arg(&uf2)
        .arg("--mount")
        .arg(&mount)
        .arg("--dry-run")
        .output()
        .expect("run flash-rp2040 dry-run");

    assert!(
        output.status.success(),
        "flash-rp2040 dry-run should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let copied = mount.join("firmware.uf2");
    assert!(!copied.exists(), "dry-run should not copy uf2");
}
