//! P2P multi-process integration tests.
//!
//! Uses `fork()` to test true cross-process SHM communication:
//! - Writer creates a segment in a child process
//! - Reader attaches from the parent process
//! - Verifies data consistency across process boundary
//! - Verifies cleanup after writer exit

use evo_common::shm::p2p::{
    ModuleAbbrev, P2pSegmentHeader, SegmentDiscovery, TypedP2pReader, TypedP2pWriter,
};
use std::time::{Duration, Instant};

/// Test payload — 128 bytes, cache-line aligned.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
struct IntegSeg {
    header: P2pSegmentHeader,
    value: u64,
    cycle: u64,
    _pad: [u8; 48],
}

/// Wait until a file appears in /dev/shm or timeout.
fn wait_for_shm(name: &str, timeout: Duration) -> bool {
    let path = format!("/dev/shm/evo_{name}");
    let start = Instant::now();
    while start.elapsed() < timeout {
        if std::path::Path::new(&path).exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

/// Wait until a file disappears from /dev/shm or timeout.
fn wait_for_shm_gone(name: &str, timeout: Duration) -> bool {
    let path = format!("/dev/shm/evo_{name}");
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !std::path::Path::new(&path).exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

/// Test: writer in child process, reader in parent process — data consistency.
///
/// 1. Fork a child process that creates a writer and writes 50 cycles.
/// 2. Parent process attaches a reader and verifies data.
/// 3. Child exits, parent verifies segment cleanup.
#[test]
fn cross_process_write_read() {
    let name = format!("integ_wr_{}", std::process::id());

    // Safety: fork() is unsafe but this is a controlled test environment.
    let pid = unsafe { libc::fork() };

    if pid == 0 {
        // ── CHILD PROCESS (writer) ──
        let mut writer =
            TypedP2pWriter::<IntegSeg>::create(&name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
                .expect("child: create writer");

        for c in 0u64..50 {
            let mut payload: IntegSeg = unsafe { core::mem::zeroed() };
            payload.value = 0xDEAD_BEEF;
            payload.cycle = c;
            writer.commit(&payload).expect("child: commit");
            std::thread::sleep(Duration::from_millis(2));
        }

        // Keep alive for a bit so parent can read.
        std::thread::sleep(Duration::from_millis(100));

        // Writer dropped here → segment cleaned up.
        drop(writer);
        std::process::exit(0);
    }

    // ── PARENT PROCESS (reader) ──
    assert!(pid > 0, "fork failed");

    // Wait for child to create the segment.
    assert!(
        wait_for_shm(&name, Duration::from_secs(5)),
        "timeout waiting for child to create segment"
    );

    // Give child time to write at least one commit.
    std::thread::sleep(Duration::from_millis(10));

    let mut reader =
        TypedP2pReader::<IntegSeg>::attach(&name, 10).expect("parent: attach reader");

    // Read a few times and verify data consistency.
    let mut max_cycle = 0u64;
    for _ in 0..10 {
        let data = reader.read().expect("parent: read");
        assert_eq!(data.value, 0xDEAD_BEEF, "data value mismatch");
        if data.cycle > max_cycle {
            max_cycle = data.cycle;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    // Verify heartbeat incremented (we read at least some cycles).
    assert!(reader.last_heartbeat() > 0, "heartbeat should be > 0");
    assert!(max_cycle > 0, "should have read at least cycle > 0");

    // Discovery should find the segment while writer is alive.
    let segs = SegmentDiscovery::list_segments();
    let found = segs.iter().find(|s| s.name == name);
    assert!(found.is_some(), "discovery should find segment while writer alive");
    assert!(found.unwrap().writer_alive, "writer should be alive");

    // Wait for child to exit.
    let mut status: libc::c_int = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }

    // After child exits, segment should be cleaned up.
    assert!(
        wait_for_shm_gone(&name, Duration::from_secs(5)),
        "segment should be cleaned up after writer exit"
    );
}

/// Test: duplicate writer rejected across processes.
///
/// 1. Parent creates a writer.
/// 2. Child attempts to create a writer for the same segment.
/// 3. Child should get WriterAlreadyExists.
#[test]
fn cross_process_duplicate_writer() {
    let name = format!("integ_dup_{}", std::process::id());

    let mut writer =
        TypedP2pWriter::<IntegSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("parent: create writer");

    let payload: IntegSeg = unsafe { core::mem::zeroed() };
    writer.commit(&payload).expect("parent: commit");

    let pid = unsafe { libc::fork() };

    if pid == 0 {
        // ── CHILD PROCESS ──
        // Attempt to create a writer for the same segment — should fail.
        let result =
            TypedP2pWriter::<IntegSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal);

        match result {
            Err(_) => {
                // Expected: WriterAlreadyExists or similar.
                std::process::exit(0);
            }
            Ok(_) => {
                // Should not succeed.
                std::process::exit(1);
            }
        }
    }

    // ── PARENT PROCESS ──
    assert!(pid > 0, "fork failed");

    let mut status: libc::c_int = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }

    // Child should have exited with code 0 (writer was rejected).
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "child should have exited successfully (duplicate rejected)"
    );
}

/// Test: reader from different process can see version hash mismatch.
///
/// 1. Parent creates a writer with IntegSeg.
/// 2. Child attempts to read with a different struct (size mismatch → version hash mismatch).
#[test]
fn cross_process_version_mismatch() {
    let name = format!("integ_ver_{}", std::process::id());

    let mut writer =
        TypedP2pWriter::<IntegSeg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
            .expect("parent: create writer");

    let payload: IntegSeg = unsafe { core::mem::zeroed() };
    writer.commit(&payload).expect("parent: commit");

    let pid = unsafe { libc::fork() };

    if pid == 0 {
        // ── CHILD PROCESS ──
        // A different struct with different size/alignment → different version hash.
        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct WrongSeg {
            header: P2pSegmentHeader,
            data: [u64; 32], // 256 bytes, vs IntegSeg's 128
        }

        match TypedP2pReader::<WrongSeg>::attach(&name, 10) {
            Ok(mut reader) => {
                // Attach may succeed (data segment is big enough due to page rounding),
                // but first read() should fail with VersionMismatch.
                match reader.read() {
                    Err(_) => std::process::exit(0), // Expected: VersionMismatch
                    Ok(_) => std::process::exit(1),  // Should not succeed
                }
            }
            Err(_) => {
                // Attach itself failed — also acceptable.
                std::process::exit(0);
            }
        }
    }

    // ── PARENT PROCESS ──
    assert!(pid > 0, "fork failed");

    let mut status: libc::c_int = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }

    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "child should have exited successfully (version mismatch detected)"
    );
}
