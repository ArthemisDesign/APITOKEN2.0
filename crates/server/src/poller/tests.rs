use super::*;

#[test]
fn codex_health_interval_has_a_ten_second_floor() {
    assert_eq!(codex_health_interval_secs(0), 10);
    assert_eq!(codex_health_interval_secs(10), 10);
    assert_eq!(codex_health_interval_secs(30), 30);
}

#[test]
fn cooling_sub_is_not_due_for_probe() {
    let now = pool::now();
    let em = "probe@test.io";
    // давно опрошена (с запасом на джиттер ≤ LIVENESS_INTERVAL/2) → созрела
    let mut l = pool::Live {
        polled_ts: now - LIVENESS_INTERVAL * 2,
        ..Default::default()
    };
    assert!(
        next_probe_at(em, &l) <= now,
        "idle-подписка должна созреть для liveness-probe"
    );
    // в cooling → НЕ probe-им до конца cooling (не долбим забаненный/лимитированный аккаунт)
    l.polled_ts = now - 310;
    l.cooling_until = now + 500;
    assert_eq!(
        next_probe_at(em, &l),
        now + 500,
        "probe откладывается до конца cooling"
    );
    assert!(next_probe_at(em, &l) > now);
}

/// Suspect/Dead — probe НЕЗАВИСИМО от cooling: их надо продолжать проверять (корроборация/
/// ресуррекция), даже пока подписка cooled после 401/403-probe. Иначе cool(900) заморозил бы вердикт.
#[test]
fn suspect_and_dead_probe_ignore_cooling() {
    let now = pool::now();
    let em = "s@test.io";
    // suspect, только что cooled(900) 401-probe-ом → всё равно созреет по SUSPECT_INTERVAL
    let suspect = pool::Live {
        polled_ts: now - SUSPECT_INTERVAL - 1,
        cooling_until: now + 900,
        auth_state: pool::AuthState::Suspect,
        ..Default::default()
    };
    assert!(
        next_probe_at(em, &suspect) <= now,
        "suspect probe-ится, не глядя на cooling"
    );
    // dead — редкий resurrection-probe по DEAD_RESURRECT_INTERVAL, тоже cooling-независимо
    let dead = pool::Live {
        polled_ts: now - DEAD_RESURRECT_INTERVAL - 1,
        cooling_until: now + 900,
        auth_state: pool::AuthState::Dead,
        ..Default::default()
    };
    assert!(
        next_probe_at(em, &dead) <= now,
        "dead получает медленный resurrection-probe"
    );
    // но dead, недавно опрошенный, НЕ созрел (медленный интервал держит редкость)
    let dead_fresh = pool::Live {
        polled_ts: now - 10,
        auth_state: pool::AuthState::Dead,
        ..Default::default()
    };
    assert!(next_probe_at(em, &dead_fresh) > now, "dead не долбим часто");
}

#[test]
fn probe_429_uses_weekly_reset_at_shared_fallback_threshold() {
    let now = 1_000_000;
    assert_eq!(
        probe_cool_secs_429(
            Some(0.99),
            Some(0.97),
            Some(now + 3600),
            Some(now + 3 * 86400),
            now,
            300,
        ),
        3 * 86400,
        "7d utilization above the forward fallback threshold must not cool only to 5h reset",
    );
}

#[test]
fn probe_429_rejects_stale_and_bounds_hostile_resets() {
    let now = 1_000_000;
    assert_eq!(
        probe_cool_secs_429(
            Some(0.99),
            Some(0.99),
            Some(now + 3600),
            Some(now - 1),
            now,
            300,
        ),
        3600,
    );
    assert_eq!(
        probe_cool_secs_429(None, Some(1.0), None, Some(i64::MAX), now, 300,),
        MAX_PROBE_COOL_SECS,
    );
}
