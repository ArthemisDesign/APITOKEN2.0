use crate::{CapacityError, Reservation};
use api_limits::ByteLimit;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageConfig {
    pub request_limit: ByteLimit,
    pub memory_threshold: ByteLimit,
}

impl StorageConfig {
    pub fn validate(self) -> Result<Self, StorageError> {
        if self.request_limit.bytes() == 0 {
            return Err(StorageError::InvalidConfig);
        }
        if self.memory_threshold > self.request_limit {
            return Err(StorageError::InvalidConfig);
        }
        self.request_limit
            .as_usize()
            .map_err(|_| StorageError::InvalidConfig)?;
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    InvalidConfig,
    TooLarge,
    ArithmeticOverflow,
    StorageExhausted,
    MemoryExhausted,
    PrivateSpoolUnavailable,
    Io,
}

pub struct PrivateSpoolFactory {
    root: File,
}

impl fmt::Debug for PrivateSpoolFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrivateSpoolFactory")
            .field("root", &"[PRIVATE]")
            .finish()
    }
}

impl PrivateSpoolFactory {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref();
        let metadata =
            std::fs::symlink_metadata(root).map_err(|_| StorageError::PrivateSpoolUnavailable)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(StorageError::PrivateSpoolUnavailable);
        }
        #[cfg(unix)]
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(StorageError::PrivateSpoolUnavailable);
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let root = options
            .open(root)
            .map_err(|_| StorageError::PrivateSpoolUnavailable)?;
        Ok(Self { root })
    }

    pub fn try_clone(&self) -> Result<Self, StorageError> {
        Ok(Self {
            root: self
                .root
                .try_clone()
                .map_err(|_| StorageError::PrivateSpoolUnavailable)?,
        })
    }

    fn create(&self) -> Result<File, StorageError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        for _ in 0..32 {
            let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let name = format!(
                ".bounded-body-{}-{now:032x}-{counter:016x}",
                std::process::id()
            );
            #[cfg(unix)]
            {
                use std::ffi::CString;
                use std::os::fd::{AsRawFd, FromRawFd};
                let name = CString::new(name).map_err(|_| StorageError::PrivateSpoolUnavailable)?;
                let fd = unsafe {
                    libc::openat(
                        self.root.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDWR
                            | libc::O_CREAT
                            | libc::O_EXCL
                            | libc::O_NOFOLLOW
                            | libc::O_CLOEXEC,
                        0o600,
                    )
                };
                if fd >= 0 {
                    if unsafe { libc::unlinkat(self.root.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                        unsafe { libc::close(fd) };
                        return Err(StorageError::PrivateSpoolUnavailable);
                    }
                    return Ok(unsafe { File::from_raw_fd(fd) });
                }
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(StorageError::PrivateSpoolUnavailable);
            }
            #[cfg(not(unix))]
            return Err(StorageError::PrivateSpoolUnavailable);
        }
        Err(StorageError::PrivateSpoolUnavailable)
    }
}

pub struct BodyStore {
    config: StorageConfig,
    factory: PrivateSpoolFactory,
    storage: Reservation,
    memory: Reservation,
    state: StoreState,
    len: u64,
}

enum StoreState {
    Memory(Vec<u8>),
    Spool(File),
    Poisoned,
}

impl fmt::Debug for BodyStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BodyStore")
            .field("len", &self.len)
            .field("kind", &self.kind())
            .finish()
    }
}

impl BodyStore {
    pub fn start(
        config: StorageConfig,
        storage_budget: &crate::Budget,
        memory_budget: &crate::Budget,
        storage: Reservation,
        memory: Reservation,
        factory: PrivateSpoolFactory,
    ) -> Result<Self, StorageError> {
        let config = config.validate()?;
        if !storage.belongs_to(storage_budget)
            || !memory.belongs_to(memory_budget)
            || std::ptr::eq(storage_budget, memory_budget)
        {
            return Err(StorageError::InvalidConfig);
        }
        Ok(Self {
            config,
            factory,
            storage,
            memory,
            state: StoreState::Memory(Vec::new()),
            len: 0,
        })
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), StorageError> {
        if matches!(self.state, StoreState::Poisoned) {
            return Err(StorageError::Io);
        }
        let chunk_len = u64::try_from(chunk.len()).map_err(|_| StorageError::ArithmeticOverflow)?;
        let next_len = self
            .len
            .checked_add(chunk_len)
            .ok_or(StorageError::ArithmeticOverflow)?;
        if next_len > self.config.request_limit.bytes() {
            return Err(StorageError::TooLarge);
        }
        match &mut self.state {
            StoreState::Memory(chunks) if next_len <= self.config.memory_threshold.bytes() => {
                self.storage
                    .try_grow_to(ByteLimit::from_bytes(next_len))
                    .map_err(storage_capacity)?;
                if let Err(error) = self.memory.try_grow_to(ByteLimit::from_bytes(next_len)) {
                    let _ = self.storage.shrink_to(ByteLimit::from_bytes(self.len));
                    return Err(memory_capacity(error));
                }
                chunks.extend_from_slice(chunk);
                self.len = next_len;
                Ok(())
            }
            StoreState::Memory(_) => self.spill_and_push(chunk, next_len),
            StoreState::Spool(file) => {
                self.storage
                    .try_grow_to(ByteLimit::from_bytes(next_len))
                    .map_err(storage_capacity)?;
                if file.write_all(chunk).is_err() {
                    let _ = self.storage.shrink_to(ByteLimit::from_bytes(self.len));
                    self.state = StoreState::Poisoned;
                    return Err(StorageError::Io);
                }
                self.len = next_len;
                Ok(())
            }
            StoreState::Poisoned => Err(StorageError::Io),
        }
    }

    pub fn finish(mut self) -> Result<StoredBody, StorageError> {
        self.storage
            .shrink_to(ByteLimit::from_bytes(self.len))
            .map_err(|_| StorageError::InvalidConfig)?;
        match &self.state {
            StoreState::Memory(_) => self
                .memory
                .shrink_to(ByteLimit::from_bytes(self.len))
                .map_err(|_| StorageError::InvalidConfig)?,
            StoreState::Spool(_) => self
                .memory
                .shrink_to(ByteLimit::from_bytes(0))
                .map_err(|_| StorageError::InvalidConfig)?,
            StoreState::Poisoned => return Err(StorageError::Io),
        }
        let state = std::mem::replace(&mut self.state, StoreState::Poisoned);
        match state {
            StoreState::Memory(bytes) => Ok(StoredBody::Memory(MemoryBody {
                bytes,
                len: self.len,
                _storage: take_reservation(&mut self.storage),
                _memory: take_reservation(&mut self.memory),
            })),
            StoreState::Spool(mut file) => {
                file.flush().map_err(|_| StorageError::Io)?;
                Ok(StoredBody::Spool(SpoolBody {
                    file,
                    len: self.len,
                    _storage: take_reservation(&mut self.storage),
                    _memory: take_reservation(&mut self.memory),
                }))
            }
            StoreState::Poisoned => Err(StorageError::Io),
        }
    }

    pub fn len(&self) -> ByteLimit {
        ByteLimit::from_bytes(self.len)
    }

    pub fn is_spooled(&self) -> bool {
        matches!(self.state, StoreState::Spool(_))
    }

    fn kind(&self) -> &'static str {
        if self.is_spooled() {
            "spool"
        } else {
            "memory"
        }
    }

    fn spill_and_push(&mut self, chunk: &[u8], next_len: u64) -> Result<(), StorageError> {
        // During spill the old in-memory prefix and the complete new disk body coexist. `next_len`
        // already includes that prefix, so the temporary storage weight is old_len + next_len.
        let transition = self
            .len
            .checked_add(next_len)
            .ok_or(StorageError::ArithmeticOverflow)?;
        self.storage
            .try_grow_to(ByteLimit::from_bytes(transition))
            .map_err(storage_capacity)?;
        let result = (|| {
            let mut file = self.factory.create()?;
            let StoreState::Memory(chunks) = &self.state else {
                return Err(StorageError::InvalidConfig);
            };
            file.write_all(chunks).map_err(|_| StorageError::Io)?;
            file.write_all(chunk).map_err(|_| StorageError::Io)?;
            file.flush().map_err(|_| StorageError::Io)?;
            Ok(file)
        })();
        let file = match result {
            Ok(file) => file,
            Err(error) => {
                let _ = self.storage.shrink_to(ByteLimit::from_bytes(self.len));
                return Err(error);
            }
        };
        let old = std::mem::replace(&mut self.state, StoreState::Spool(file));
        drop(old);
        self.memory
            .shrink_to(ByteLimit::from_bytes(0))
            .map_err(|_| StorageError::InvalidConfig)?;
        self.storage
            .shrink_to(ByteLimit::from_bytes(next_len))
            .map_err(|_| StorageError::InvalidConfig)?;
        self.len = next_len;
        Ok(())
    }
}

// Reservations cannot be default-constructed. Moving them out of a drop-bearing store uses a
// zero reservation from the exact same budget, preserving single-owner accounting.
fn take_reservation(reservation: &mut Reservation) -> Reservation {
    reservation.take()
}

fn storage_capacity(error: CapacityError) -> StorageError {
    match error {
        CapacityError::ArithmeticOverflow => StorageError::ArithmeticOverflow,
        CapacityError::Exhausted => StorageError::StorageExhausted,
    }
}

fn memory_capacity(error: CapacityError) -> StorageError {
    match error {
        CapacityError::ArithmeticOverflow => StorageError::ArithmeticOverflow,
        CapacityError::Exhausted => StorageError::MemoryExhausted,
    }
}

pub enum StoredBody {
    Memory(MemoryBody),
    Spool(SpoolBody),
}

pub struct StoredBodyLease {
    _storage: Reservation,
    _memory: Reservation,
}

impl fmt::Debug for StoredBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredBody")
            .field("len", &self.len())
            .field(
                "kind",
                &if matches!(self, Self::Spool(_)) {
                    "spool"
                } else {
                    "memory"
                },
            )
            .finish()
    }
}

impl StoredBody {
    pub fn len(&self) -> ByteLimit {
        match self {
            Self::Memory(body) => ByteLimit::from_bytes(body.len),
            Self::Spool(body) => ByteLimit::from_bytes(body.len),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len().bytes() == 0
    }

    pub fn into_memory(self) -> Result<(Vec<u8>, StoredBodyLease), Self> {
        match self {
            Self::Memory(body) => Ok((
                body.bytes,
                StoredBodyLease {
                    _storage: body._storage,
                    _memory: body._memory,
                },
            )),
            spool @ Self::Spool(_) => Err(spool),
        }
    }

    pub fn copy_to(&mut self, writer: &mut impl Write) -> Result<(), StorageError> {
        match self {
            Self::Memory(body) => writer.write_all(&body.bytes).map_err(|_| StorageError::Io),
            Self::Spool(body) => {
                body.file
                    .seek(SeekFrom::Start(0))
                    .map_err(|_| StorageError::Io)?;
                std::io::copy(&mut body.file, writer).map_err(|_| StorageError::Io)?;
                Ok(())
            }
        }
    }
}

pub struct MemoryBody {
    bytes: Vec<u8>,
    len: u64,
    _storage: Reservation,
    _memory: Reservation,
}

pub struct SpoolBody {
    file: File,
    len: u64,
    _storage: Reservation,
    _memory: Reservation,
}

impl fmt::Debug for MemoryBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryBody")
            .field("len", &self.len)
            .finish()
    }
}

impl fmt::Debug for SpoolBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpoolBody")
            .field("len", &self.len)
            .finish()
    }
}

#[cfg(test)]
mod tests;
