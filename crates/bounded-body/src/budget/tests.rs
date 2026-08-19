use super::*;

fn bytes(value: u64) -> ByteLimit {
    ByteLimit::from_bytes(value)
}

#[test]
fn rounds_weights_and_fails_fast_without_waiting() {
    let budget = Budget::new(bytes(10), bytes(4)).unwrap();
    let one = budget.try_reserve(bytes(3)).unwrap();
    assert_eq!(one.bytes().bytes(), 4);
    let two = budget.try_reserve(bytes(4)).unwrap();
    assert_eq!(budget.used_bytes(), 8);
    assert_eq!(
        budget.try_reserve(bytes(1)).unwrap_err(),
        CapacityError::Exhausted
    );
    drop(one);
    assert_eq!(budget.used_bytes(), 4);
    drop(two);
    assert_eq!(budget.used_bytes(), 0);
}

#[test]
fn growth_failure_preserves_old_reservation_and_shrink_releases_delta() {
    let budget = Budget::new(bytes(8), bytes(1)).unwrap();
    let mut first = budget.try_reserve(bytes(4)).unwrap();
    let blocker = budget.try_reserve(bytes(4)).unwrap();
    assert_eq!(first.try_grow_to(bytes(5)), Err(CapacityError::Exhausted));
    assert_eq!(first.bytes().bytes(), 4);
    drop(blocker);
    first.try_grow_to(bytes(7)).unwrap();
    assert_eq!(budget.used_bytes(), 7);
    first.shrink_to(bytes(2)).unwrap();
    assert_eq!(budget.used_bytes(), 2);
    assert_eq!(
        first.shrink_to(bytes(3)),
        Err(ReservationError::CannotGrowWithShrink)
    );
}

#[test]
fn replace_and_drop_release_exactly_once_on_error_and_panic() {
    let budget = Budget::new(bytes(16), bytes(2)).unwrap();
    {
        let mut reservation = budget.try_reserve(bytes(3)).unwrap();
        reservation.try_replace(bytes(9)).unwrap();
        assert_eq!(budget.used_bytes(), 10);
        reservation.try_replace(bytes(1)).unwrap();
        assert_eq!(budget.used_bytes(), 2);
    }
    assert_eq!(budget.used_bytes(), 0);

    let caught = std::panic::catch_unwind({
        let budget = budget.clone();
        move || {
            let _reservation = budget.try_reserve(bytes(7)).unwrap();
            panic!("test panic");
        }
    });
    assert!(caught.is_err());
    assert_eq!(budget.used_bytes(), 0);
}

#[test]
fn invalid_and_overflowing_configuration_fails_closed() {
    assert_eq!(
        Budget::new(bytes(0), bytes(1)).unwrap_err(),
        BudgetConfigError::ZeroCapacity
    );
    assert_eq!(
        Budget::new(bytes(1), bytes(0)).unwrap_err(),
        BudgetConfigError::ZeroUnit
    );
    assert_eq!(
        Budget::new(bytes(1), bytes(2)).unwrap_err(),
        BudgetConfigError::ZeroCapacity
    );
    let budget = Budget::new(bytes(16), bytes(2)).unwrap();
    assert_eq!(
        budget.try_reserve(bytes(u64::MAX)).unwrap_err(),
        CapacityError::ArithmeticOverflow
    );
}

#[test]
fn storage_and_rss_are_independent_authorities() {
    let storage = Budget::new(bytes(8), bytes(1)).unwrap();
    let rss = Budget::new(bytes(4), bytes(1)).unwrap();
    let _storage = storage.try_reserve(bytes(8)).unwrap();
    let _rss = rss.try_reserve(bytes(4)).unwrap();
    assert_eq!(
        storage.try_reserve(bytes(1)).unwrap_err(),
        CapacityError::Exhausted
    );
    assert_eq!(
        rss.try_reserve(bytes(1)).unwrap_err(),
        CapacityError::Exhausted
    );
}
