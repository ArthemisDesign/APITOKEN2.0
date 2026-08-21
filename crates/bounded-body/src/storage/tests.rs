use super::*;
use crate::Budget;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn bytes(value: u64) -> ByteLimit {
    ByteLimit::from_bytes(value)
}

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "bounded-body-test-{}-{}-{suffix}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self { path }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn store(root: &TestRoot, limit: u64, threshold: u64) -> (BodyStore, Budget, Budget) {
    let storage = Budget::new(bytes(limit * 3), bytes(1)).unwrap();
    let memory = Budget::new(bytes(limit), bytes(1)).unwrap();
    let storage_reservation = storage.try_reserve(bytes(0)).unwrap();
    let memory_reservation = memory.try_reserve(bytes(0)).unwrap();
    let factory = PrivateSpoolFactory::new(&root.path).unwrap();
    (
        BodyStore::start(
            StorageConfig {
                request_limit: bytes(limit),
                memory_threshold: bytes(threshold),
            },
            &storage,
            &memory,
            storage_reservation,
            memory_reservation,
            factory,
        )
        .unwrap(),
        storage,
        memory,
    )
}

fn replay(body: &mut StoredBody) -> Vec<u8> {
    let mut output = Vec::new();
    body.copy_to(&mut output).unwrap();
    output
}

#[test]
fn reservations_must_belong_to_distinct_declared_authorities() {
    let root = TestRoot::new();
    let storage = Budget::new(bytes(16), bytes(1)).unwrap();
    let memory = Budget::new(bytes(16), bytes(1)).unwrap();
    let foreign = Budget::new(bytes(16), bytes(1)).unwrap();
    let config = StorageConfig {
        request_limit: bytes(8),
        memory_threshold: bytes(4),
    };
    assert_eq!(
        BodyStore::start(
            config,
            &storage,
            &memory,
            foreign.try_reserve(bytes(0)).unwrap(),
            memory.try_reserve(bytes(0)).unwrap(),
            PrivateSpoolFactory::new(&root.path).unwrap(),
        )
        .unwrap_err(),
        StorageError::InvalidConfig
    );
    assert_eq!(
        BodyStore::start(
            config,
            &storage,
            &storage,
            storage.try_reserve(bytes(0)).unwrap(),
            storage.try_reserve(bytes(0)).unwrap(),
            PrivateSpoolFactory::new(&root.path).unwrap(),
        )
        .unwrap_err(),
        StorageError::InvalidConfig
    );
}

#[test]
fn threshold_boundaries_and_one_byte_chunks_replay_exactly() {
    for size in [3usize, 4, 5] {
        let root = TestRoot::new();
        let (mut store, storage, memory) = store(&root, 8, 4);
        for byte in 0..size {
            store.push(&[byte as u8]).unwrap();
        }
        assert_eq!(store.is_spooled(), size > 4);
        let mut body = store.finish().unwrap();
        assert_eq!(replay(&mut body), (0..size as u8).collect::<Vec<_>>());
        assert_eq!(body.len().bytes(), size as u64);
        assert!(root.path.read_dir().unwrap().next().is_none());
        drop(body);
        assert_eq!(storage.used_bytes(), 0);
        assert_eq!(memory.used_bytes(), 0);
    }
}

#[test]
fn request_limit_rejects_before_copy_or_write_and_keeps_prior_body() {
    let root = TestRoot::new();
    let (mut store, storage, memory) = store(&root, 5, 2);
    store.push(b"12345").unwrap();
    assert_eq!(store.push(b"6"), Err(StorageError::TooLarge));
    let mut body = store.finish().unwrap();
    assert_eq!(replay(&mut body), b"12345");
    drop(body);
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(memory.used_bytes(), 0);
}

#[test]
fn declared_pre_reservation_is_consumed_without_double_acquire() {
    let root = TestRoot::new();
    let storage = Budget::new(bytes(16), bytes(1)).unwrap();
    let memory = Budget::new(bytes(16), bytes(1)).unwrap();
    let mut store = BodyStore::start(
        StorageConfig {
            request_limit: bytes(8),
            memory_threshold: bytes(8),
        },
        &storage,
        &memory,
        storage.try_reserve(bytes(8)).unwrap(),
        memory.try_reserve(bytes(8)).unwrap(),
        PrivateSpoolFactory::new(&root.path).unwrap(),
    )
    .unwrap();
    store.push(b"declared").unwrap();
    assert_eq!(storage.used_bytes(), 8);
    assert_eq!(memory.used_bytes(), 8);
    let mut body = store.finish().unwrap();
    assert_eq!(replay(&mut body), b"declared");
}

#[test]
fn memory_exhaustion_rolls_back_new_storage_weight() {
    let root = TestRoot::new();
    let storage = Budget::new(bytes(8), bytes(1)).unwrap();
    let memory = Budget::new(bytes(2), bytes(1)).unwrap();
    let mut store = BodyStore::start(
        StorageConfig {
            request_limit: bytes(8),
            memory_threshold: bytes(8),
        },
        &storage,
        &memory,
        storage.try_reserve(bytes(0)).unwrap(),
        memory.try_reserve(bytes(0)).unwrap(),
        PrivateSpoolFactory::new(&root.path).unwrap(),
    )
    .unwrap();
    assert_eq!(store.push(b"123"), Err(StorageError::MemoryExhausted));
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(memory.used_bytes(), 0);
    assert_eq!(store.len().bytes(), 0);
}

#[test]
fn drop_before_and_after_spill_releases_both_authorities() {
    let root = TestRoot::new();
    let (mut memory_store, storage, memory) = store(&root, 8, 4);
    memory_store.push(b"123").unwrap();
    drop(memory_store);
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(memory.used_bytes(), 0);

    let (mut spool_store, storage, memory) = store(&root, 8, 4);
    spool_store.push(b"12345").unwrap();
    assert!(spool_store.is_spooled());
    assert_eq!(memory.used_bytes(), 0);
    drop(spool_store);
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(memory.used_bytes(), 0);
}

#[test]
fn spill_reserves_the_old_prefix_and_complete_disk_body_during_transition() {
    let root = TestRoot::new();
    let storage = Budget::new(bytes(8), bytes(1)).unwrap();
    let memory = Budget::new(bytes(8), bytes(1)).unwrap();
    let mut store = BodyStore::start(
        StorageConfig {
            request_limit: bytes(8),
            memory_threshold: bytes(4),
        },
        &storage,
        &memory,
        storage.try_reserve(bytes(0)).unwrap(),
        memory.try_reserve(bytes(0)).unwrap(),
        PrivateSpoolFactory::new(&root.path).unwrap(),
    )
    .unwrap();
    store.push(b"1234").unwrap();
    // The complete 8-byte disk body would coexist with the 4-byte memory prefix. An 8-byte
    // storage authority must refuse before writing rather than leave that prefix unaccounted.
    assert_eq!(store.push(b"5678"), Err(StorageError::StorageExhausted));
    assert!(!store.is_spooled());
    assert_eq!(storage.used_bytes(), 4);
}

#[test]
fn storage_exhaustion_is_fail_fast_before_transition() {
    let root = TestRoot::new();
    let storage = Budget::new(bytes(4), bytes(1)).unwrap();
    let memory = Budget::new(bytes(8), bytes(1)).unwrap();
    let mut store = BodyStore::start(
        StorageConfig {
            request_limit: bytes(8),
            memory_threshold: bytes(2),
        },
        &storage,
        &memory,
        storage.try_reserve(bytes(0)).unwrap(),
        memory.try_reserve(bytes(0)).unwrap(),
        PrivateSpoolFactory::new(&root.path).unwrap(),
    )
    .unwrap();
    store.push(b"12").unwrap();
    assert_eq!(store.push(b"345"), Err(StorageError::StorageExhausted));
    assert!(!store.is_spooled());
    assert_eq!(store.len().bytes(), 2);
}

#[test]
fn opened_directory_handle_survives_path_replacement_without_redirecting_spool() {
    let root = TestRoot::new();
    let factory = PrivateSpoolFactory::new(&root.path).unwrap();
    let moved = root.path.with_extension("moved");
    std::fs::rename(&root.path, &moved).unwrap();
    std::fs::create_dir(&root.path).unwrap();
    std::fs::set_permissions(&root.path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let storage = Budget::new(bytes(32), bytes(1)).unwrap();
    let memory = Budget::new(bytes(16), bytes(1)).unwrap();
    let mut store = BodyStore::start(
        StorageConfig {
            request_limit: bytes(8),
            memory_threshold: bytes(2),
        },
        &storage,
        &memory,
        storage.try_reserve(bytes(0)).unwrap(),
        memory.try_reserve(bytes(0)).unwrap(),
        factory,
    )
    .unwrap();
    store.push(b"spooled").unwrap();
    assert!(root.path.read_dir().unwrap().next().is_none());
    assert!(moved.read_dir().unwrap().next().is_none());
    drop(store);
    std::fs::remove_dir(&root.path).unwrap();
    std::fs::rename(&moved, &root.path).unwrap();
}

#[test]
fn private_root_and_debug_contract_fail_closed() {
    let root = TestRoot::new();
    std::fs::set_permissions(&root.path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        PrivateSpoolFactory::new(&root.path).unwrap_err(),
        StorageError::PrivateSpoolUnavailable
    );
    let secret = "secret-body-material";
    let root = TestRoot::new();
    let (mut store, _, _) = store(&root, 64, 4);
    store.push(secret.as_bytes()).unwrap();
    let debug = format!("{store:?}");
    assert!(!debug.contains(secret));
    assert!(!debug.contains(root.path.to_string_lossy().as_ref()));
}

#[test]
fn panic_unwind_cleans_anonymous_spool_and_reservations() {
    let root = TestRoot::new();
    let (mut store, storage, memory) = store(&root, 16, 2);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.push(b"spooled").unwrap();
        panic!("test panic");
    }));
    assert!(result.is_err());
    drop(store);
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(memory.used_bytes(), 0);
    assert!(root.path.read_dir().unwrap().next().is_none());
}

#[test]
fn spilled_body_reloads_into_bytes_and_tracks_live_files() {
    let root = TestRoot::new();
    let factory = PrivateSpoolFactory::new(&root.path).unwrap();
    assert_eq!(factory.live_files(), 0);
    let storage = Budget::new(bytes(32), bytes(1)).unwrap();
    let memory = Budget::new(bytes(16), bytes(1)).unwrap();
    let mut store = BodyStore::start(
        StorageConfig {
            request_limit: bytes(8),
            memory_threshold: bytes(2),
        },
        &storage,
        &memory,
        storage.try_reserve(bytes(0)).unwrap(),
        memory.try_reserve(bytes(0)).unwrap(),
        factory.try_clone().unwrap(),
    )
    .unwrap();
    store.push(b"spilled!").unwrap();
    assert!(store.is_spooled());
    assert_eq!(factory.live_files(), 1);
    assert_eq!(memory.used_bytes(), 0);
    let body = store.finish().unwrap();
    assert_eq!(factory.live_files(), 1);
    let (bytes, lease) = body.into_bytes().unwrap();
    assert_eq!(bytes, b"spilled!");
    assert_eq!(factory.live_files(), 0);
    assert!(memory.used_bytes() >= 8);
    drop(lease);
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(memory.used_bytes(), 0);
    assert!(root.path.read_dir().unwrap().next().is_none());
}
