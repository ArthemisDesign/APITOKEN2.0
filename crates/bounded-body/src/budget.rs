use api_limits::ByteLimit;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct Budget {
    inner: Arc<Inner>,
}

struct Inner {
    capacity_units: u64,
    unit_bytes: u64,
    used_units: AtomicU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetConfigError {
    ZeroCapacity,
    ZeroUnit,
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityError {
    ArithmeticOverflow,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservationError {
    CannotGrowWithShrink,
}

impl fmt::Debug for Budget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Budget")
            .field("capacity_bytes", &self.capacity_bytes())
            .field("used_bytes", &self.used_bytes())
            .finish()
    }
}

impl Budget {
    pub fn new(capacity: ByteLimit, unit: ByteLimit) -> Result<Self, BudgetConfigError> {
        if capacity.bytes() == 0 {
            return Err(BudgetConfigError::ZeroCapacity);
        }
        if unit.bytes() == 0 {
            return Err(BudgetConfigError::ZeroUnit);
        }
        let capacity_units = capacity.bytes() / unit.bytes();
        if capacity_units == 0 {
            return Err(BudgetConfigError::ZeroCapacity);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                capacity_units,
                unit_bytes: unit.bytes(),
                used_units: AtomicU64::new(0),
            }),
        })
    }

    pub fn try_reserve(&self, weight: ByteLimit) -> Result<Reservation, CapacityError> {
        let units = units_for_capacity(weight.bytes(), self.inner.unit_bytes)?;
        self.acquire(units)?;
        Ok(Reservation {
            budget: self.clone(),
            units,
        })
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.inner
            .capacity_units
            .saturating_mul(self.inner.unit_bytes)
    }

    pub fn used_bytes(&self) -> u64 {
        self.inner
            .used_units
            .load(Ordering::Acquire)
            .saturating_mul(self.inner.unit_bytes)
    }

    fn acquire(&self, units: u64) -> Result<(), CapacityError> {
        if units == 0 {
            return Ok(());
        }
        self.inner
            .used_units
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                let next = used.checked_add(units)?;
                (next <= self.inner.capacity_units).then_some(next)
            })
            .map(|_| ())
            .map_err(|_| CapacityError::Exhausted)
    }

    fn release(&self, units: u64) {
        if units == 0 {
            return;
        }
        let previous = self.inner.used_units.fetch_sub(units, Ordering::AcqRel);
        debug_assert!(previous >= units, "budget reservation underflow");
    }
}

pub struct Reservation {
    budget: Budget,
    units: u64,
}

impl fmt::Debug for Reservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reservation")
            .field("bytes", &self.bytes())
            .finish()
    }
}

impl Reservation {
    pub fn bytes(&self) -> ByteLimit {
        ByteLimit::from_bytes(self.units.saturating_mul(self.budget.inner.unit_bytes))
    }

    pub fn belongs_to(&self, budget: &Budget) -> bool {
        Arc::ptr_eq(&self.budget.inner, &budget.inner)
    }

    pub fn try_grow_to(&mut self, weight: ByteLimit) -> Result<(), CapacityError> {
        let target = units_for_capacity(weight.bytes(), self.budget.inner.unit_bytes)?;
        if target <= self.units {
            return Ok(());
        }
        let additional = target - self.units;
        self.budget.acquire(additional)?;
        self.units = target;
        Ok(())
    }

    pub fn shrink_to(&mut self, weight: ByteLimit) -> Result<(), ReservationError> {
        let target = units_for_capacity(weight.bytes(), self.budget.inner.unit_bytes)
            .map_err(|_| ReservationError::CannotGrowWithShrink)?;
        if target > self.units {
            return Err(ReservationError::CannotGrowWithShrink);
        }
        self.budget.release(self.units - target);
        self.units = target;
        Ok(())
    }

    pub fn try_replace(&mut self, weight: ByteLimit) -> Result<(), CapacityError> {
        let target = units_for_capacity(weight.bytes(), self.budget.inner.unit_bytes)?;
        if target > self.units {
            self.budget.acquire(target - self.units)?;
        } else {
            self.budget.release(self.units - target);
        }
        self.units = target;
        Ok(())
    }

    pub(crate) fn take(&mut self) -> Self {
        let units = self.units;
        self.units = 0;
        Self {
            budget: self.budget.clone(),
            units,
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.release(self.units);
        self.units = 0;
    }
}

fn units_for_capacity(bytes: u64, unit: u64) -> Result<u64, CapacityError> {
    if bytes == 0 {
        return Ok(0);
    }
    bytes
        .checked_add(unit - 1)
        .ok_or(CapacityError::ArithmeticOverflow)
        .map(|rounded| rounded / unit)
}

#[cfg(test)]
mod tests;
