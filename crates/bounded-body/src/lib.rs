//! Runtime-independent fail-fast admission and private bounded body storage.

mod budget;
mod storage;

pub use budget::{Budget, BudgetConfigError, CapacityError, Reservation, ReservationError};
pub use storage::{
    BodyStore, MemoryBody, PrivateSpoolFactory, SpoolBody, StorageConfig, StorageError, StoredBody,
};
