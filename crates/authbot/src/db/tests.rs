use super::*;

fn tmp() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = format!(
        "{}/authbot_test_{}_{}",
        std::env::temp_dir().display(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let _ = std::fs::remove_dir_all(&dir);
    format!("{dir}/authbot.db")
}

fn lifecycle_conflict(error: &anyhow::Error) -> Option<&ProxyLifecycleConflict> {
    error.downcast_ref::<ProxyLifecycleConflict>()
}

fn inventory_id(marker: char) -> String {
    format!("inv_{}", marker.to_string().repeat(32))
}

fn selection(marker: char, order_id: i64, allocation_ip: &str) -> RenewalSelection {
    RenewalSelection {
        inventory_id: inventory_id(marker),
        order_id,
        allocation_ip: allocation_ip.parse().unwrap(),
        allow_inactive_subscription: false,
    }
}

fn insert_legacy_pending_request(s: &Store, key: &str, selection: &RenewalSelection) -> i64 {
    let (
        selections,
        inventory_ids,
        order_ids,
        encoded_selections,
        encoded_inventory_ids,
        encoded_order_ids,
    ) = canonical_selections(std::slice::from_ref(selection)).unwrap();
    assert_eq!(selections, [selection.clone()]);
    let c = s.c.lock().unwrap();
    c.execute(
        "INSERT INTO proxy_renewal_requests(idempotency_key,selections,inventory_ids,order_ids,
                                            requested_by,state,created_at,updated_at)
         VALUES(?1,?2,?3,?4,'legacy-admin','pending',?5,?5)",
        rusqlite::params![
            key,
            encoded_selections,
            encoded_inventory_ids,
            encoded_order_ids,
            now()
        ],
    )
    .unwrap();
    assert_eq!(inventory_ids, vec![selection.inventory_id.clone()]);
    assert_eq!(order_ids, vec![selection.order_id]);
    c.last_insert_rowid()
}

#[test]
fn proxy_bindings_allow_multi_ip_order_and_keep_stable_ids_across_reopen() {
    let p = tmp();
    let first_id;
    {
        let s = Store::open(&p).unwrap();
        let first = s
            .upsert_proxy_binding_allocation(
                "gemini",
                "profile-a",
                41,
                "2001:0db8::1",
                100,
                ProxyAuthorityStatus::Local,
            )
            .unwrap();
        let second = s
            .upsert_proxy_binding_allocation(
                "gemini",
                "profile-b",
                41,
                "2001:db8::2",
                101,
                ProxyAuthorityStatus::Local,
            )
            .unwrap();
        assert_eq!(first.allocation_ip.unwrap().to_string(), "2001:db8::1");
        assert_ne!(first.inventory_id, second.inventory_id);
        first_id = first.inventory_id.clone();
        let replay = s
            .upsert_proxy_binding_allocation(
                "gemini",
                "profile-a",
                41,
                "2001:db8::1",
                999,
                ProxyAuthorityStatus::Unknown,
            )
            .unwrap();
        assert_eq!(replay.inventory_id, first_id);
        assert_eq!(replay.issued_at, 100);
        let duplicate = s
            .upsert_proxy_binding_allocation(
                "kimi",
                "profile-c",
                41,
                "2001:db8::1",
                102,
                ProxyAuthorityStatus::Local,
            )
            .unwrap_err();
        assert_eq!(
            lifecycle_conflict(&duplicate),
            Some(&ProxyLifecycleConflict::OrderAlreadyBound)
        );
    }
    let reopened = Store::open(&p).unwrap();
    assert_eq!(
        reopened
            .get_proxy_binding_by_inventory_id(&first_id)
            .unwrap()
            .unwrap()
            .inventory_id,
        first_id
    );
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn codex_exact_upsert_adopts_only_an_exact_legacy_gpt_binding() {
    let s = Store::open(":memory:").unwrap();
    let legacy = s
        .upsert_proxy_binding_allocation(
            "gpt",
            "account-a",
            51,
            "192.0.2.51",
            101,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();

    let codex = s
        .upsert_proxy_binding_allocation(
            "codex",
            "account-a",
            51,
            "192.0.2.51",
            999,
            ProxyAuthorityStatus::Unknown,
        )
        .unwrap();

    assert_eq!(codex.provider, "codex");
    assert_eq!(codex.inventory_id, legacy.inventory_id);
    assert_eq!(codex.issued_at, 101);
    assert_eq!(codex.authority_status, ProxyAuthorityStatus::Unknown);
    assert_eq!(s.list_proxy_bindings().unwrap(), vec![codex]);
}

#[test]
fn codex_upsert_does_not_adopt_mismatched_unresolved_or_shadowed_gpt_bindings() {
    let s = Store::open(":memory:").unwrap();
    let mismatched = s
        .upsert_proxy_binding_allocation(
            "gpt",
            "mismatch",
            61,
            "192.0.2.61",
            201,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();
    let unresolved = s
        .upsert_proxy_binding("gpt", "unresolved", 62, 202, ProxyAuthorityStatus::Unknown)
        .unwrap();
    let shadowed = s
        .upsert_proxy_binding_allocation(
            "gpt",
            "shadowed",
            63,
            "192.0.2.63",
            203,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();
    let existing_codex = s
        .upsert_proxy_binding_allocation(
            "codex",
            "shadowed",
            64,
            "192.0.2.64",
            204,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();

    let mismatch_codex = s
        .upsert_proxy_binding_allocation(
            "codex",
            "mismatch",
            65,
            "192.0.2.65",
            205,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();
    let unresolved_codex = s
        .upsert_proxy_binding_allocation(
            "codex",
            "unresolved",
            62,
            "192.0.2.62",
            206,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();
    let replayed_codex = s
        .upsert_proxy_binding_allocation(
            "codex",
            "shadowed",
            64,
            "192.0.2.64",
            999,
            ProxyAuthorityStatus::Unknown,
        )
        .unwrap();

    let bindings = s.list_proxy_bindings().unwrap();
    assert!(bindings.iter().any(|binding| binding == &mismatched));
    assert!(bindings.iter().any(|binding| binding == &unresolved));
    assert!(bindings.iter().any(|binding| binding == &shadowed));
    assert_ne!(mismatch_codex.inventory_id, mismatched.inventory_id);
    assert_ne!(unresolved_codex.inventory_id, unresolved.inventory_id);
    assert_eq!(replayed_codex.inventory_id, existing_codex.inventory_id);
    assert_eq!(replayed_codex.issued_at, existing_codex.issued_at);
}

#[test]
fn codex_upsert_never_adopts_ambiguous_legacy_gpt_rows() {
    let s = Store::open(":memory:").unwrap();
    {
        let c = s.c.lock().unwrap();
        c.execute_batch(
            "DROP TABLE proxy_bindings;
             CREATE TABLE proxy_bindings(
                inventory_id TEXT NOT NULL, provider TEXT NOT NULL, local_id TEXT NOT NULL,
                order_id INTEGER NOT NULL, allocation_ip TEXT, issued_at INTEGER NOT NULL,
                authority_status TEXT NOT NULL, updated_at INTEGER NOT NULL);
             INSERT INTO proxy_bindings VALUES
                ('inv_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','gpt','duplicate',71,
                 '192.0.2.71',301,'local',301),
                ('inv_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','gpt','duplicate',71,
                 '192.0.2.71',302,'local',302);",
        )
        .unwrap();
    }

    let error = s
        .upsert_proxy_binding_allocation(
            "codex",
            "duplicate",
            71,
            "192.0.2.71",
            303,
            ProxyAuthorityStatus::Local,
        )
        .unwrap_err();
    assert_eq!(
        lifecycle_conflict(&error),
        Some(&ProxyLifecycleConflict::OrderAlreadyBound)
    );
    assert_eq!(
        s.list_proxy_bindings()
            .unwrap()
            .iter()
            .filter(|binding| binding.provider == "gpt")
            .count(),
        2
    );
}

#[test]
fn exact_request_snapshot_allows_duplicate_order_for_different_allocations() {
    let s = Store::open(":memory:").unwrap();
    let a = selection('a', 7, "192.0.2.1");
    let b = selection('b', 7, "192.0.2.2");
    let first = s
        .create_or_get_renewal_request_exact(
            "exact-request",
            &[b.clone(), a.clone()],
            "first-admin",
        )
        .unwrap();
    assert_eq!(
        first.inventory_ids,
        vec![a.inventory_id.clone(), b.inventory_id.clone()]
    );
    assert_eq!(first.order_ids, vec![7, 7]);
    let replay = s
        .create_or_get_renewal_request_exact(
            "exact-request",
            &[a.clone(), b.clone()],
            "other-admin",
        )
        .unwrap();
    assert_eq!(replay, first);
    let conflict = s
        .create_or_get_renewal_request_exact(
            "exact-request",
            &[a.clone(), selection('b', 7, "192.0.2.3")],
            "third-admin",
        )
        .unwrap_err();
    assert_eq!(
        lifecycle_conflict(&conflict),
        Some(&ProxyLifecycleConflict::IdempotencyKeyReused)
    );
    assert!(s
        .create_or_get_renewal_request_exact(
            "duplicate-inventory",
            &[a.clone(), a.clone()],
            "admin",
        )
        .is_err());
    assert!(s
        .create_or_get_renewal_request_exact(
            "duplicate-exact-allocation",
            &[a, selection('c', 7, "192.0.2.1")],
            "admin",
        )
        .is_err());
}

#[test]
fn renewal_creation_rejects_pending_and_in_progress_overlap_without_queueing() {
    let s = Store::open(":memory:").unwrap();
    let queued_selection = selection('a', 81, "198.51.100.81");
    let queued = s
        .create_or_get_renewal_request_exact("queued-overlap", &[queued_selection.clone()], "admin")
        .unwrap();

    let inventory_error = s
        .create_or_get_renewal_request_exact(
            "inventory-overlap",
            &[RenewalSelection {
                inventory_id: queued_selection.inventory_id.clone(),
                order_id: 82,
                allocation_ip: "198.51.100.82".parse().unwrap(),
                allow_inactive_subscription: false,
            }],
            "admin",
        )
        .unwrap_err();
    assert_eq!(
        lifecycle_conflict(&inventory_error),
        Some(&ProxyLifecycleConflict::RenewalSelectionBusy)
    );
    assert!(s
        .get_renewal_request_by_key("inventory-overlap")
        .unwrap()
        .is_none());

    s.claim_renewal_request(queued.id).unwrap().unwrap();
    let allocation_error = s
        .create_or_get_renewal_request_exact(
            "allocation-overlap",
            &[RenewalSelection {
                inventory_id: inventory_id('b'),
                order_id: queued_selection.order_id,
                allocation_ip: queued_selection.allocation_ip,
                allow_inactive_subscription: false,
            }],
            "admin",
        )
        .unwrap_err();
    assert_eq!(
        lifecycle_conflict(&allocation_error),
        Some(&ProxyLifecycleConflict::RenewalSelectionBusy)
    );
    assert!(s
        .get_renewal_request_by_key("allocation-overlap")
        .unwrap()
        .is_none());

    let replay = s
        .create_or_get_renewal_request_exact("queued-overlap", &[queued_selection], "other-admin")
        .unwrap();
    assert_eq!(replay, s.get_renewal_request(queued.id).unwrap().unwrap());
}

#[test]
fn direct_claim_terminalizes_legacy_overlap_sibling_and_preserves_disjoint() {
    let s = Store::open(":memory:").unwrap();
    let winner_selection = selection('c', 83, "198.51.100.83");
    let sibling_selection = RenewalSelection {
        inventory_id: inventory_id('d'),
        order_id: winner_selection.order_id,
        allocation_ip: winner_selection.allocation_ip,
        allow_inactive_subscription: false,
    };
    let disjoint_selection = selection('e', 84, "198.51.100.84");
    let winner_id = insert_legacy_pending_request(&s, "legacy-direct-winner", &winner_selection);
    let sibling_id = insert_legacy_pending_request(&s, "legacy-direct-sibling", &sibling_selection);
    let disjoint_id =
        insert_legacy_pending_request(&s, "legacy-direct-disjoint", &disjoint_selection);

    let winner = s.claim_renewal_request(winner_id).unwrap().unwrap();
    assert_eq!(winner.state, RenewalRequestState::InProgress);
    assert_eq!(
        s.get_renewal_request(sibling_id).unwrap().unwrap().state,
        RenewalRequestState::Indeterminate
    );
    assert_eq!(
        s.get_renewal_request(disjoint_id).unwrap().unwrap().state,
        RenewalRequestState::Pending
    );
    assert!(s.claim_renewal_request(sibling_id).unwrap().is_none());
    assert_eq!(
        s.claim_renewal_request(disjoint_id).unwrap().unwrap().state,
        RenewalRequestState::InProgress
    );
}

#[test]
fn disjoint_renewal_requests_can_be_claimed_together() {
    let s = Store::open(":memory:").unwrap();
    let first = s
        .create_or_get_renewal_request_exact(
            "disjoint-first",
            &[selection('c', 91, "203.0.113.91")],
            "admin",
        )
        .unwrap();
    let second = s
        .create_or_get_renewal_request_exact(
            "disjoint-second",
            &[selection('d', 92, "203.0.113.92")],
            "admin",
        )
        .unwrap();

    assert_eq!(
        s.claim_renewal_request(first.id).unwrap().unwrap().state,
        RenewalRequestState::InProgress
    );
    assert_eq!(
        s.claim_renewal_request(second.id).unwrap().unwrap().state,
        RenewalRequestState::InProgress
    );
}

#[test]
fn background_claim_chooses_oldest_legacy_winner_and_terminalizes_overlap() {
    let s = Store::open(":memory:").unwrap();
    let winner_selection = selection('f', 101, "192.0.2.101");
    let sibling_selection = RenewalSelection {
        inventory_id: inventory_id('g'),
        order_id: winner_selection.order_id,
        allocation_ip: winner_selection.allocation_ip,
        allow_inactive_subscription: false,
    };
    let disjoint_selection = selection('h', 102, "192.0.2.102");
    let winner_id = insert_legacy_pending_request(&s, "background-winner", &winner_selection);
    let sibling_id = insert_legacy_pending_request(&s, "background-sibling", &sibling_selection);
    let disjoint_id = insert_legacy_pending_request(&s, "background-disjoint", &disjoint_selection);

    assert_eq!(
        s.claim_next_renewal_request().unwrap().unwrap().id,
        winner_id
    );
    assert_eq!(
        s.get_renewal_request(sibling_id).unwrap().unwrap().state,
        RenewalRequestState::Indeterminate
    );
    assert_eq!(
        s.claim_next_renewal_request().unwrap().unwrap().id,
        disjoint_id
    );
    assert!(s.claim_renewal_request(sibling_id).unwrap().is_none());
    assert!(s.claim_next_renewal_request().unwrap().is_none());
}

#[test]
fn renewal_events_are_inventory_identified_for_two_allocations_of_one_order() {
    let s = Store::open(":memory:").unwrap();
    let a = selection('a', 11, "198.51.100.1");
    let b = selection('b', 11, "198.51.100.2");
    let request = s
        .create_or_get_renewal_request_exact("event-request", &[a.clone(), b.clone()], "admin")
        .unwrap();
    s.claim_renewal_request(request.id).unwrap().unwrap();
    assert!(s
        .record_renewal_event(request.id, 11, RenewalEventOutcome::Renewed, 100, Some(200),)
        .is_err());
    let event = s
        .record_renewal_event_for_inventory(
            request.id,
            &a.inventory_id,
            RenewalEventOutcome::Renewed,
            100,
            Some(200),
        )
        .unwrap();
    assert_eq!(
        s.record_renewal_event_for_inventory(
            request.id,
            &a.inventory_id,
            RenewalEventOutcome::Renewed,
            100,
            Some(200),
        )
        .unwrap(),
        event
    );
    let changed = s
        .record_renewal_event_for_inventory(
            request.id,
            &a.inventory_id,
            RenewalEventOutcome::Rejected,
            100,
            None,
        )
        .unwrap_err();
    assert_eq!(
        lifecycle_conflict(&changed),
        Some(&ProxyLifecycleConflict::RenewalEventChanged)
    );
    assert!(s.complete_renewal_request(request.id).is_err());
    s.record_renewal_event_for_inventory(
        request.id,
        &b.inventory_id,
        RenewalEventOutcome::Unchanged,
        101,
        None,
    )
    .unwrap();
    assert_eq!(
        s.complete_renewal_request(request.id).unwrap().state,
        RenewalRequestState::Completed
    );
    let events = s.get_exact_renewal_events(request.id).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events
            .iter()
            .map(|event| (
                event.inventory_id.as_str(),
                event.event.order_id,
                event.allocation_ip,
            ))
            .collect::<Vec<_>>(),
        vec![
            (a.inventory_id.as_str(), 11, "198.51.100.1".parse().unwrap()),
            (b.inventory_id.as_str(), 11, "198.51.100.2".parse().unwrap())
        ]
    );
}

#[test]
fn exact_snapshot_and_actor_survive_restart_while_in_progress_is_fenced() {
    let p = tmp();
    let request_id;
    let pending_id;
    {
        let s = Store::open(&p).unwrap();
        let request = s
            .create_or_get_renewal_request_exact(
                "restart-request",
                &[selection('r', 71, "203.0.113.71")],
                "ops@example.com/primary",
            )
            .unwrap();
        request_id = request.id;
        s.claim_renewal_request(request_id).unwrap().unwrap();
        pending_id = s
            .create_or_get_renewal_request_exact(
                "pending-replay",
                &[selection('p', 72, "203.0.113.72")],
                "ops@example.com/primary",
            )
            .unwrap()
            .id;
    }
    let reopened = Store::open(&p).unwrap();
    let request = reopened.get_renewal_request(request_id).unwrap().unwrap();
    assert_eq!(request.state, RenewalRequestState::Indeterminate);
    assert_eq!(request.requested_by, "ops@example.com/primary");
    assert_eq!(request.inventory_ids, vec![inventory_id('r')]);
    assert_eq!(request.order_ids, vec![71]);
    assert_eq!(
        reopened.get_renewal_selections(request_id).unwrap(),
        vec![selection('r', 71, "203.0.113.71")]
    );
    let pending = reopened.claim_next_renewal_request().unwrap().unwrap();
    assert_eq!(pending.id, pending_id);
    assert_eq!(
        reopened.get_renewal_selections(pending_id).unwrap(),
        vec![selection('p', 72, "203.0.113.72")]
    );
    assert!(reopened.claim_next_renewal_request().unwrap().is_none());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn legacy_binding_rebuild_preserves_identity_and_exact_backfill() {
    let p = tmp();
    let parent = std::path::Path::new(&p).parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let legacy_id = inventory_id('l');
    {
        let c = Connection::open(&p).unwrap();
        c.execute_batch(&format!(
            "CREATE TABLE proxy_bindings(
                inventory_id TEXT, provider TEXT NOT NULL, local_id TEXT NOT NULL,
                order_id INTEGER NOT NULL UNIQUE, issued_at INTEGER NOT NULL,
                authority_status TEXT NOT NULL, updated_at INTEGER NOT NULL,
                PRIMARY KEY(provider,local_id));
             INSERT INTO proxy_bindings VALUES(
                '{legacy_id}','gemini','legacy-profile',91,11,'local',12);
             CREATE TABLE proxy_renewal_requests(
                id INTEGER PRIMARY KEY AUTOINCREMENT, idempotency_key TEXT NOT NULL UNIQUE,
                order_ids TEXT NOT NULL, state TEXT NOT NULL,
                created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             INSERT INTO proxy_renewal_requests(
                idempotency_key,order_ids,state,created_at,updated_at)
             VALUES('legacy-pending','91','pending',1,1),
                   ('legacy-completed','91','completed',1,1);"
        ))
        .unwrap();
    }
    let s = Store::open(&p).unwrap();
    let legacy = s.list_proxy_bindings().unwrap().remove(0);
    assert_eq!(legacy.inventory_id, legacy_id);
    assert_eq!(legacy.issued_at, 11);
    assert!(legacy.allocation_ip.is_none());
    let backfilled = s
        .upsert_proxy_binding_allocation(
            "gemini",
            "legacy-profile",
            91,
            "192.0.2.91",
            99,
            ProxyAuthorityStatus::Local,
        )
        .unwrap();
    assert_eq!(backfilled.inventory_id, legacy_id);
    assert_eq!(backfilled.issued_at, 11);
    assert_eq!(backfilled.allocation_ip.unwrap().to_string(), "192.0.2.91");
    assert_eq!(
        s.get_renewal_request_by_key("legacy-pending")
            .unwrap()
            .unwrap()
            .state,
        RenewalRequestState::Indeterminate
    );
    assert_eq!(
        s.get_renewal_request_by_key("legacy-completed")
            .unwrap()
            .unwrap()
            .state,
        RenewalRequestState::Completed
    );
    drop(s);
    assert_eq!(
        Store::open(&p).unwrap().list_proxy_bindings().unwrap()[0].inventory_id,
        legacy_id
    );
    let _ = std::fs::remove_dir_all(parent);
}

#[test]
fn previous_proxy_lifecycle_schema_rebuild_preserves_contract_and_is_idempotent() {
    type RequestRow = (
        i64,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        i64,
        i64,
    );
    type EventRow = (
        i64,
        i64,
        Option<String>,
        i64,
        Option<String>,
        String,
        i64,
        Option<i64>,
    );

    fn lifecycle_rows(c: &Connection) -> (Vec<RequestRow>, Vec<EventRow>) {
        let requests = {
            let mut statement = c
                .prepare(
                    "SELECT id,idempotency_key,selections,inventory_ids,order_ids,requested_by,
                            state,created_at,updated_at
                     FROM proxy_renewal_requests ORDER BY id",
                )
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        let events = {
            let mut statement = c
                .prepare(
                    "SELECT id,request_id,inventory_id,order_id,allocation_ip,outcome,
                            observed_at,new_expiry_at
                     FROM proxy_renewal_events ORDER BY id",
                )
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        (requests, events)
    }

    fn lifecycle_sequences(c: &Connection) -> (i64, i64) {
        let sequence = |table| {
            c.query_row(
                "SELECT seq FROM sqlite_sequence WHERE name=?1",
                rusqlite::params![table],
                |row| row.get(0),
            )
            .unwrap()
        };
        (
            sequence("proxy_renewal_requests"),
            sequence("proxy_renewal_events"),
        )
    }

    fn assert_lifecycle_schema(c: &Connection) {
        let foreign_key = c
            .query_row("PRAGMA foreign_key_list(proxy_renewal_events)", [], |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .unwrap();
        assert_eq!(
            foreign_key,
            (
                "proxy_renewal_requests".to_string(),
                "request_id".to_string(),
                "id".to_string(),
            )
        );
        let mut foreign_key_check = c.prepare("PRAGMA foreign_key_check").unwrap();
        assert!(foreign_key_check
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());

        let indexes = c
            .prepare(
                "SELECT name,sql FROM sqlite_master
                 WHERE type='index' AND name IN (
                    'proxy_bindings_authority_idx',
                    'proxy_renewal_requests_state_idx',
                    'proxy_renewal_events_request_idx')
                 ORDER BY name",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            indexes,
            vec![
                (
                    "proxy_bindings_authority_idx".to_string(),
                    "CREATE INDEX proxy_bindings_authority_idx\n            ON proxy_bindings(authority_status,provider,local_id)".to_string(),
                ),
                (
                    "proxy_renewal_events_request_idx".to_string(),
                    "CREATE INDEX proxy_renewal_events_request_idx\n            ON proxy_renewal_events(request_id,id)".to_string(),
                ),
                (
                    "proxy_renewal_requests_state_idx".to_string(),
                    "CREATE INDEX proxy_renewal_requests_state_idx\n            ON proxy_renewal_requests(state,created_at,id)".to_string(),
                ),
            ]
        );
        let event_schema = c
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type='table' AND name='proxy_renewal_events'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert!(event_schema.contains("'local_profile_inactive'"));
    }

    let path = tmp();
    let parent = std::path::Path::new(&path).parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let first = selection('a', 91, "192.0.2.91");
    let second = selection('b', 92, "192.0.2.92");
    let fresh = selection('c', 93, "192.0.2.93");
    let (_, _, _, first_selections, first_ids, first_orders) =
        canonical_selections(std::slice::from_ref(&first)).unwrap();
    let (_, _, _, second_selections, second_ids, second_orders) =
        canonical_selections(std::slice::from_ref(&second)).unwrap();
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE proxy_bindings(
                inventory_id TEXT NOT NULL UNIQUE CHECK(length(inventory_id) BETWEEN 1 AND 160),
                provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 64),
                local_id TEXT NOT NULL CHECK(length(local_id) BETWEEN 1 AND 255),
                order_id INTEGER NOT NULL CHECK(order_id > 0), allocation_ip TEXT,
                issued_at INTEGER NOT NULL CHECK(issued_at > 0),
                authority_status TEXT NOT NULL CHECK(authority_status IN ('local','unknown')),
                updated_at INTEGER NOT NULL CHECK(updated_at > 0),
                PRIMARY KEY(provider,local_id), UNIQUE(order_id,allocation_ip));
             CREATE INDEX proxy_bindings_authority_idx
                ON proxy_bindings(authority_status,provider,local_id);
             CREATE TABLE proxy_renewal_requests(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                idempotency_key TEXT NOT NULL UNIQUE CHECK(length(idempotency_key) BETWEEN 1 AND 255),
                selections TEXT CHECK(length(selections) BETWEEN 1 AND 32768),
                inventory_ids TEXT CHECK(length(inventory_ids) BETWEEN 1 AND 16384),
                order_ids TEXT NOT NULL CHECK(length(order_ids) BETWEEN 1 AND 8192),
                requested_by TEXT NOT NULL CHECK(length(requested_by) BETWEEN 1 AND 128),
                state TEXT NOT NULL CHECK(state IN
                    ('pending','in_progress','completed','failed','indeterminate')),
                created_at INTEGER NOT NULL CHECK(created_at > 0),
                updated_at INTEGER NOT NULL CHECK(updated_at > 0));
             CREATE INDEX proxy_renewal_requests_state_idx
                ON proxy_renewal_requests(state,created_at,id);
             CREATE TABLE proxy_renewal_events(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id INTEGER NOT NULL REFERENCES proxy_renewal_requests(id),
                inventory_id TEXT, order_id INTEGER NOT NULL CHECK(order_id > 0), allocation_ip TEXT,
                outcome TEXT NOT NULL CHECK(outcome IN
                    ('renewed','unchanged','not_found','rejected','provider_unavailable','indeterminate')),
                observed_at INTEGER NOT NULL CHECK(observed_at > 0),
                new_expiry_at INTEGER CHECK(new_expiry_at IS NULL OR new_expiry_at > 0),
                UNIQUE(request_id,inventory_id));
             CREATE INDEX proxy_renewal_events_request_idx
                ON proxy_renewal_events(request_id,id);",
        )
        .unwrap();
        for (local_id, item) in [
            ("legacy-a", &first),
            ("legacy-b", &second),
            ("fresh", &fresh),
        ] {
            c.execute(
                "INSERT INTO proxy_bindings VALUES(?1,'codex',?2,?3,?4,10,'local',11)",
                rusqlite::params![
                    item.inventory_id,
                    local_id,
                    item.order_id,
                    item.allocation_ip.to_string()
                ],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO proxy_renewal_requests VALUES(7,'legacy-renewed',?1,?2,?3,
                                                        'legacy-admin','completed',20,21)",
            rusqlite::params![first_selections, first_ids, first_orders],
        )
        .unwrap();
        c.execute(
            "INSERT INTO proxy_renewal_requests VALUES(19,'legacy-rejected',?1,?2,?3,
                                                         'legacy-admin','failed',22,23)",
            rusqlite::params![second_selections, second_ids, second_orders],
        )
        .unwrap();
        c.execute(
            "INSERT INTO proxy_renewal_events VALUES(11,7,?1,91,'192.0.2.91',
                                                       'renewed',30,1000)",
            rusqlite::params![first.inventory_id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO proxy_renewal_events VALUES(27,19,?1,92,'192.0.2.92',
                                                       'rejected',31,NULL)",
            rusqlite::params![second.inventory_id],
        )
        .unwrap();
        c.execute(
            "INSERT INTO proxy_renewal_requests VALUES(41,'deleted-request',?1,?2,?3,
                                                         'legacy-admin','failed',24,25)",
            rusqlite::params![first_selections, first_ids, first_orders],
        )
        .unwrap();
        c.execute(
            "INSERT INTO proxy_renewal_events VALUES(53,41,?1,91,'192.0.2.91',
                                                       'rejected',32,NULL)",
            rusqlite::params![first.inventory_id],
        )
        .unwrap();
        c.execute("DELETE FROM proxy_renewal_events WHERE id=53", [])
            .unwrap();
        c.execute("DELETE FROM proxy_renewal_requests WHERE id=41", [])
            .unwrap();
        assert_eq!(lifecycle_sequences(&c), (41, 53));
    }

    let original = lifecycle_rows(&Connection::open(&path).unwrap());
    let store = Store::open(&path).unwrap();
    {
        let c = store.c.lock().unwrap();
        assert_eq!(lifecycle_rows(&c), original);
        assert_eq!(lifecycle_sequences(&c), (41, 53));
        assert_lifecycle_schema(&c);
    }
    assert_eq!(
        serde_json::to_value(RenewalEventOutcome::LocalProfileInactive).unwrap(),
        serde_json::json!("local_profile_inactive")
    );
    assert_eq!(
        serde_json::from_value::<RenewalEventOutcome>(serde_json::json!("local_profile_inactive"))
            .unwrap(),
        RenewalEventOutcome::LocalProfileInactive
    );

    let request = store
        .create_or_get_renewal_request_exact("fresh-local-inactive", &[fresh.clone()], "admin")
        .unwrap();
    assert!(request.id > 41);
    store.claim_renewal_request(request.id).unwrap().unwrap();
    let event = store
        .record_renewal_event_for_inventory(
            request.id,
            &fresh.inventory_id,
            RenewalEventOutcome::LocalProfileInactive,
            40,
            None,
        )
        .unwrap();
    assert!(event.id > 53);
    assert_eq!(event.outcome, RenewalEventOutcome::LocalProfileInactive);
    store.fail_renewal_request(request.id).unwrap();
    let (after_upgrade, upgraded_sequences) = {
        let c = store.c.lock().unwrap();
        (lifecycle_rows(&c), lifecycle_sequences(&c))
    };
    assert_eq!(upgraded_sequences, (request.id, event.id));
    drop(store);

    let reopened = Store::open(&path).unwrap();
    {
        let c = reopened.c.lock().unwrap();
        assert_eq!(lifecycle_rows(&c), after_upgrade);
        assert_eq!(lifecycle_sequences(&c), upgraded_sequences);
        assert_lifecycle_schema(&c);
    }
    assert_eq!(
        reopened
            .get_exact_renewal_events(request.id)
            .unwrap()
            .remove(0)
            .event
            .outcome,
        RenewalEventOutcome::LocalProfileInactive
    );
    drop(reopened);
    let _ = std::fs::remove_dir_all(parent);
}

#[test]
fn parked_gemini_verification_is_generation_fenced_and_counts_every_press() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    let job = SellerJobRef {
        kind: "offer".into(),
        offer_id: 7,
        batch_id: 0,
        item_no: 0,
        token: "generation-a".into(),
    };
    assert!(!s.gemini_verification_is_parked(555).unwrap());
    s.park_gemini_verification(
        555,
        "sealed-envelope",
        now() + 3600,
        now() + 1800,
        now() - 1,
        Some(&job),
    )
    .unwrap();
    assert!(s.gemini_verification_is_parked(555).unwrap());
    // Reading the record must not consume an attempt: it only decides whether to show a button.
    assert!(s.gemini_verification_is_parked(555).unwrap());

    // Each claim is one paid acceptance generation, so the counter advances per press and the
    // job generation travels with the record.
    let first = s
        .claim_gemini_verification(555, now() + 300)
        .unwrap()
        .unwrap();
    assert_eq!(first.attempts, 1);
    assert_eq!(first.sealed_payload, "sealed-envelope");
    assert_eq!(first.job.as_ref().unwrap().token, "generation-a");
    assert_eq!(
        s.claim_gemini_verification(555, now() + 300)
            .unwrap()
            .unwrap()
            .attempts,
        2
    );

    // Re-parking a newer account for the same seller restarts the count instead of stacking.
    s.park_gemini_verification(
        555,
        "second-envelope",
        now() + 3600,
        now() + 1800,
        now() - 1,
        Some(&job),
    )
    .unwrap();
    assert_eq!(
        s.claim_gemini_verification(555, now() + 300)
            .unwrap()
            .unwrap()
            .attempts,
        1
    );

    // An expired record is swept rather than served: parked token material is not permanent.
    s.park_gemini_verification(
        555,
        "stale-envelope",
        now() - 1,
        now() - 1,
        now() - 1,
        Some(&job),
    )
    .unwrap();
    assert!(!s.gemini_verification_is_parked(555).unwrap());
    assert!(s
        .claim_gemini_verification(555, now() + 300)
        .unwrap()
        .is_none());

    s.park_gemini_verification(
        555,
        "sealed-envelope",
        now() + 3600,
        now() + 1800,
        now() - 1,
        Some(&job),
    )
    .unwrap();
    s.clear_gemini_verification(555).unwrap();
    assert!(s
        .claim_gemini_verification(555, now() + 300)
        .unwrap()
        .is_none());
}

/// The automatic acceptance window: who the sweep may probe, how a claim spaces the next
/// attempt, and that a terminal verdict stops probing WITHOUT discarding the credential — the
/// exact combination that turned one throttled proxy into a permanently dead retry button.
#[test]
fn gemini_probe_schedule_bounds_the_sweep_and_keeps_the_credential() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    let job = SellerJobRef {
        kind: "offer".into(),
        offer_id: 9,
        batch_id: 0,
        item_no: 0,
        token: "generation-b".into(),
    };

    // Freshly recorded: the first automatic attempt is one interval away, not immediate.
    s.park_gemini_verification(
        777,
        "sealed",
        now() + 86_400,
        now() + 3600,
        now() + 300,
        Some(&job),
    )
    .unwrap();
    assert!(s.due_gemini_verifications().unwrap().is_empty());

    // Due once its next probe time has passed; claiming pushes the following attempt out, so a
    // manual press and the sweep cannot double-charge one account.
    s.schedule_gemini_probe(777, now() - 1, None, "generation_unavailable")
        .unwrap();
    assert_eq!(s.due_gemini_verifications().unwrap(), vec![777]);
    s.claim_gemini_verification(777, now() + 300)
        .unwrap()
        .unwrap();
    assert!(s.due_gemini_verifications().unwrap().is_empty());

    // A terminal verdict stops the sweep but leaves the sealed material readable.
    s.schedule_gemini_probe(777, 0, Some(0), "duplicate_account")
        .unwrap();
    assert!(s.due_gemini_verifications().unwrap().is_empty());
    assert!(s.gemini_verification_is_parked(777).unwrap());
    assert_eq!(
        s.claim_gemini_verification(777, now() + 300)
            .unwrap()
            .unwrap()
            .sealed_payload,
        "sealed"
    );

    // Re-sealing swaps only the material; schedule and fence survive.
    assert!(s.reseal_gemini_verification(777, "resealed").unwrap());
    let claimed = s
        .claim_gemini_verification(777, now() + 300)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.sealed_payload, "resealed");
    assert_eq!(claimed.job.as_ref().unwrap().token, "generation-b");

    // A closed window notifies exactly once, and the record stays.
    s.park_gemini_verification(
        778,
        "sealed",
        now() + 86_400,
        now() - 1,
        now() - 1,
        Some(&job),
    )
    .unwrap();
    assert!(s.due_gemini_verifications().unwrap().is_empty());
    assert_eq!(s.expired_gemini_probe_windows().unwrap(), vec![778]);
    assert!(s.mark_gemini_probe_window_notified(778).unwrap());
    assert!(!s.mark_gemini_probe_window_notified(778).unwrap());
    assert!(s.expired_gemini_probe_windows().unwrap().is_empty());
    assert!(s.gemini_verification_is_parked(778).unwrap());
}

#[test]
fn state_survives_restart() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        s.set_status(111, "approved").unwrap();
        s.set_want(111, "ho_code").unwrap();
        s.register_user(222, 222, "gemini-seller").unwrap();
        s.set_want(222, "gm_proxy").unwrap();
        s.register_user(333, 333, "legacy-gemini-seller").unwrap();
        s.set_want(333, "gm_auth").unwrap();
        s.set_admin_state(999, "seller", "Claude Max20x", 0)
            .unwrap(); // «в процессе» создания оффера
        s.set_admin_state(999, "price", "Claude Max20x", 111)
            .unwrap(); // продавец выбран
        let oid = s.create_offer("Claude Max20x", "$20", 999, 111).unwrap();
        assert_eq!(oid, 1);
    }
    // «рестарт» бота = новое открытие той же БД
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_interrupted_handoffs().unwrap(), 1);
    assert_eq!(s.recover_legacy_gemini_handoffs().unwrap(), 2);
    assert_eq!(s.get_user(111).unwrap().unwrap().status, "approved");
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "ho_email");
    assert_eq!(s.get_user(222).unwrap().unwrap().want, "gm_gproxy");
    assert_eq!(s.get_user(333).unwrap().unwrap().want, "gm_gproxy");
    s.set_hproxy_order(222, 42).unwrap();
    s.start_gemini_oauth(222, "pending-state", "sealed", now() + 60, 0)
        .unwrap();
    // Незавершённая транзакция переживает рестарт и остаётся видимой для шага назад,
    // а номер IPRoyal-заказа не теряется.
    assert!(s.pending_gemini_session(222).unwrap().is_some());
    assert_eq!(s.get_user(222).unwrap().unwrap().hproxy_order, 42);
    assert_eq!(s.approved_sellers().unwrap(), vec![111]);
    // машина создания оффера НЕ потеряна (это и был баг Python-версии)
    let (step, product, seller) = s.get_admin_state(999).unwrap().unwrap();
    assert_eq!(step, "price");
    assert_eq!(product, "Claude Max20x");
    assert_eq!(seller, 111);
    let o = s.get_offer(1).unwrap().unwrap();
    assert_eq!(o.product, "Claude Max20x");
    assert_eq!(o.seller_chat, 111);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Дочерний процесс device-флоу не переживает рестарт, а его одноразовый код истекает без
/// присмотра. Продавец обязан вернуться на шаг email, иначе он ждёт подтверждения, которое
/// больше никто не опрашивает.
#[test]
fn restart_returns_a_waiting_codex_seller_to_the_email_step() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "codex-seller").unwrap();
        s.set_want(111, "cx_wait").unwrap();
        s.set_hproxy(111, "http://user:pass@1.2.3.4:8080").unwrap();
        s.register_user(222, 222, "codex-seller-at-email").unwrap();
        s.set_want(222, "cx_email").unwrap();
    }
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_interrupted_codex_handoffs().unwrap(), 1);
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "cx_email");
    // Прокси сохраняется: повторный device-флоу обязан уйти с того же egress.
    assert_eq!(
        s.get_user(111).unwrap().unwrap().hproxy,
        "http://user:pass@1.2.3.4:8080"
    );
    // Продавец, уже стоящий на шаге email, не трогается.
    assert_eq!(s.get_user(222).unwrap().unwrap().want, "cx_email");
    assert_eq!(s.recover_interrupted_codex_handoffs().unwrap(), 0);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Валидация ключа GLM живёт только в памяти, а сам ключ нигде не персистится, поэтому
/// рестарт посреди `glm_wait` его теряет. Продавец возвращается на шаг подтверждения и
/// присылает ключ заново; прокси и выбор площадки при этом обязаны пережить рестарт.
#[test]
fn restart_returns_a_waiting_glm_seller_to_the_ready_step() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "glm-seller").unwrap();
        s.set_want(111, "glm_wait").unwrap();
        s.set_hproxy(111, "http://user:pass@1.2.3.4:8080").unwrap();
        s.set_hregion(111, "cn").unwrap();
        s.register_user(222, 222, "glm-seller-at-ready").unwrap();
        s.set_want(222, "glm_ready").unwrap();
    }
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_interrupted_glm_handoffs().unwrap(), 1);
    let user = s.get_user(111).unwrap().unwrap();
    assert_eq!(user.want, "glm_ready");
    // Прокси и площадка сохраняются: повторная валидация уйдёт с того же egress на ту же
    // площадку.
    assert_eq!(user.hproxy, "http://user:pass@1.2.3.4:8080");
    assert_eq!(user.hregion, "cn");
    // Продавец, уже стоящий на шаге подтверждения, не трогается.
    assert_eq!(s.get_user(222).unwrap().unwrap().want, "glm_ready");
    assert_eq!(s.recover_interrupted_glm_handoffs().unwrap(), 0);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Та же дисциплина, что и у GLM: валидация ключа Tripo3D живёт только в памяти, а сам ключ
/// нигде не персистится, поэтому рестарт посреди `t3_wait` его теряет. Продавец возвращается
/// на шаг подтверждения и присылает ключ заново; прокси и выбор площадки переживают рестарт.
#[test]
fn restart_returns_a_waiting_tripo3d_seller_to_the_ready_step() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "tripo3d-seller").unwrap();
        s.set_want(111, "t3_wait").unwrap();
        s.set_hproxy(111, "http://user:pass@1.2.3.4:8080").unwrap();
        s.set_hregion(111, "cn").unwrap();
        s.register_user(222, 222, "tripo3d-seller-at-ready").unwrap();
        s.set_want(222, "t3_ready").unwrap();
    }
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_interrupted_tripo3d_handoffs().unwrap(), 1);
    let user = s.get_user(111).unwrap().unwrap();
    assert_eq!(user.want, "t3_ready");
    // Прокси и площадка сохраняются: повторная валидация уйдёт с того же egress на ту же
    // площадку.
    assert_eq!(user.hproxy, "http://user:pass@1.2.3.4:8080");
    assert_eq!(user.hregion, "cn");
    // Продавец, уже стоящий на шаге подтверждения, не трогается.
    assert_eq!(s.get_user(222).unwrap().unwrap().want, "t3_ready");
    assert_eq!(s.recover_interrupted_tripo3d_handoffs().unwrap(), 0);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Та же дисциплина, что и у GLM: валидация сессии Suno живёт только в памяти, а сама cookie
/// нигде не персистится, поэтому рестарт посреди `su_wait` её теряет. Продавец возвращается
/// на шаг подтверждения и присылает cookie заново; прокси переживает рестарт.
#[test]
fn restart_returns_a_waiting_suno_seller_to_the_ready_step() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "suno-seller").unwrap();
        s.set_want(111, "su_wait").unwrap();
        s.set_hproxy(111, "http://user:pass@1.2.3.4:8080").unwrap();
        s.register_user(222, 222, "suno-seller-at-ready").unwrap();
        s.set_want(222, "su_ready").unwrap();
    }
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_interrupted_suno_handoffs().unwrap(), 1);
    let user = s.get_user(111).unwrap().unwrap();
    assert_eq!(user.want, "su_ready");
    // Прокси сохраняется: повторная валидация уйдёт с того же egress.
    assert_eq!(user.hproxy, "http://user:pass@1.2.3.4:8080");
    // Продавец, уже стоящий на шаге подтверждения, не трогается.
    assert_eq!(s.get_user(222).unwrap().unwrap().want, "su_ready");
    assert_eq!(s.recover_interrupted_suno_handoffs().unwrap(), 0);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn batch_has_one_proxy_per_item_and_advances_atomically() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    let proxies = vec![
        "http://u1:p1@1.1.1.1:1001".to_string(),
        "http://u2:p2@2.2.2.2:1002".to_string(),
        "http://u3:p3@3.3.3.3:1003".to_string(),
    ];
    let id = s
        .create_batch("ChatGPT Plus", "$20", 3, "$60", 999, 111, "buyer", &proxies)
        .unwrap();
    assert_eq!(s.get_batch(id).unwrap().unwrap().status, "offered");
    assert_eq!(s.batch_items(id).unwrap().len(), 3);
    assert_eq!(s.get_batch_item(id, 2).unwrap().unwrap().proxy, proxies[1]);
    assert!(!s.start_batch_item(id, 2).unwrap());
    assert!(s.accept_batch(id, 111).unwrap());
    assert!(s.claim_batch_payment(id).unwrap());
    s.mark_batch_paid(id, "0xtest").unwrap();
    let resume = s.batches_needing_resume().unwrap();
    assert_eq!(resume.len(), 1);
    assert_eq!(resume[0].1.item_no, 1);
    assert!(s.start_batch_item(id, 1).unwrap());
    assert!(!s.start_batch_item(id, 3).unwrap());
    assert_eq!(
        s.active_batch_for_seller(111)
            .unwrap()
            .unwrap()
            .current_item,
        1
    );

    let first_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
    let first = s.finish_batch_item(id, 1, &first_token).unwrap().unwrap();
    assert_eq!(
        first,
        BatchCompletion {
            batch_id: id,
            item_no: 1,
            total: 3,
            completed: false
        }
    );
    assert!(s.start_batch_item(id, 2).unwrap());
    assert_eq!(
        s.get_batch_item(id, 2).unwrap().unwrap().status,
        "processing"
    );
    let second_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
    assert!(
        !s.finish_batch_item(id, 2, &second_token)
            .unwrap()
            .unwrap()
            .completed
    );
    assert!(s.start_batch_item(id, 3).unwrap());
    let third_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
    assert!(
        s.finish_batch_item(id, 3, &third_token)
            .unwrap()
            .unwrap()
            .completed
    );
    assert!(s.active_batch_for_seller(111).unwrap().is_none());
    assert!(s.finish_batch_item(id, 3, &third_token).unwrap().is_none());

    let queued = s
        .create_batch("ChatGPT Plus", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(queued, 111).unwrap());
    assert!(s.claim_batch_payment(queued).unwrap());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn seller_cannot_accept_two_batches_at_once() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    let first = s
        .create_batch("Claude Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    let second = s
        .create_batch("Claude Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(first, 111).unwrap());
    assert!(!s.accept_batch(second, 111).unwrap());
    let accepted = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(accepted.reference.batch_id, first);
    assert_eq!(accepted.phase, "accepted");
    assert!(s.claim_batch_payment(first).unwrap());
    assert_eq!(s.batches_needing_payment_review().unwrap().len(), 1);
    assert!(!s.claim_batch_payment(second).unwrap());
    assert!(s.reset_batch_payment(first).unwrap());
    assert_eq!(s.get_batch(first).unwrap().unwrap().status, "accepted");
    assert_eq!(s.active_seller_job(111).unwrap().unwrap().phase, "accepted");
    assert!(s.claim_batch_payment(first).unwrap());
    s.mark_batch_paid(first, "0xfirst").unwrap();
    assert!(!s.claim_batch_payment(second).unwrap());
    assert!(s.start_batch_item(first, 1).unwrap());
    assert!(!s.claim_batch_payment(second).unwrap());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn seller_reservation_starts_at_acceptance_across_single_and_batch() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller-one").unwrap();
    let offer = s
        .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
        .unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_offer(offer, 111, 111).unwrap());
    assert!(!s.accept_batch(batch, 111).unwrap());
    let job = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(job.reference.kind, "offer");
    assert_eq!(job.reference.offer_id, offer);
    assert_eq!(job.phase, "accepted");

    s.register_user(222, 222, "seller-two").unwrap();
    let batch = s
        .create_batch("Claude Pro", "$20", 2, "$40", 999, 222, "seller", &[])
        .unwrap();
    let offer = s
        .create_offer_with_proxy("Claude Pro", "$20", 999, 222, "seller", "")
        .unwrap();
    assert!(s.accept_batch(batch, 222).unwrap());
    assert!(!s.accept_offer(offer, 222, 222).unwrap());
    let job = s.active_seller_job(222).unwrap().unwrap();
    assert_eq!(job.reference.kind, "batch");
    assert_eq!(job.reference.batch_id, batch);
    assert_eq!(job.phase, "accepted");
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn single_offer_and_batch_share_one_exact_seller_lock() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let offer = s
        .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
        .unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    s.set_response(offer, 111, "accepted").unwrap();

    assert!(!s.claim_offer_payment(offer, 111).unwrap());
    assert_eq!(
        s.response_status(offer, 111).unwrap().as_deref(),
        Some("accepted")
    );
    let job = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(job.reference.kind, "batch");
    assert_eq!(job.reference.batch_id, batch);
    assert_eq!(job.reference.item_no, 1);
    assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
    assert_eq!(
        s.get_batch_item(batch, 1).unwrap().unwrap().status,
        "processing"
    );
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn unrelated_single_completion_cannot_advance_a_batch() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let offer = s
        .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
        .unwrap();
    s.set_response(offer, 111, "paid").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());

    assert!(!s.finish_offer_job(111, offer, "stale-offer").unwrap());
    assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
    assert_eq!(
        s.get_batch_item(batch, 1).unwrap().unwrap().status,
        "processing"
    );
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn archiving_an_accepted_offer_is_exact_and_audit_safe() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let offer = s
        .create_offer_with_proxy("Claude Pro", "$20", 999, 111, "seller", "")
        .unwrap();
    assert!(s.accept_offer(offer, 111, 111).unwrap());
    s.set_want(111, "reg_address").unwrap();
    let job = s.active_seller_job(111).unwrap().unwrap();

    assert!(s
        .archive_offer(offer, 111, "stale-generation", 999)
        .unwrap()
        .is_none());
    assert_eq!(s.active_seller_job(111).unwrap().unwrap(), job);
    assert_eq!(
        s.archive_offer(offer, 111, &job.reference.token, 999)
            .unwrap()
            .as_deref(),
        Some("accepted")
    );
    assert!(s.active_seller_job(111).unwrap().is_none());
    assert_eq!(
        s.response_status(offer, 111).unwrap().as_deref(),
        Some("cancelled")
    );
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "");
    assert!(s.get_offer(offer).unwrap().is_some());
    assert_eq!(s.recover_seller_jobs().unwrap(), 0);
    assert!(!s.accept_offer(offer, 111, 111).unwrap());

    let audit =
        s.c.lock()
            .unwrap()
            .query_row(
                "SELECT seller_chat,seller_uid,response_status,job_phase,archived_by
             FROM offer_archive_events WHERE offer_id=?1",
                rusqlite::params![offer],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .unwrap();
    assert_eq!(audit, (111, 111, "accepted".into(), "accepted".into(), 999));
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn offer_archive_blocks_uncertain_payment_but_can_stop_paid_handoff() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let offer = s
        .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
        .unwrap();
    assert!(s.accept_offer(offer, 111, 111).unwrap());
    assert!(s.claim_offer_payment(offer, 111).unwrap());
    let paying = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(paying.phase, "paying");
    assert!(s
        .archive_offer(offer, 111, &paying.reference.token, 999)
        .unwrap()
        .is_none());
    assert_eq!(
        s.response_status(offer, 111).unwrap().as_deref(),
        Some("paying")
    );
    let audit_count: i64 =
        s.c.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM offer_archive_events", [], |row| {
                row.get(0)
            })
            .unwrap();
    assert_eq!(audit_count, 0);

    assert!(s.reset_offer_payment(offer, 111).unwrap());
    assert!(s.claim_offer_payment(offer, 111).unwrap());
    assert!(s.mark_offer_paid(offer, 111).unwrap());
    let processing = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(processing.phase, "processing");
    assert_eq!(
        s.archive_offer(offer, 111, &processing.reference.token, 999)
            .unwrap()
            .as_deref(),
        Some("processing")
    );
    assert!(s.active_seller_job(111).unwrap().is_none());
    assert_eq!(
        s.response_status(offer, 111).unwrap().as_deref(),
        Some("cancelled")
    );
    let audit =
        s.c.lock()
            .unwrap()
            .query_row(
                "SELECT response_status,job_phase,archived_by
             FROM offer_archive_events WHERE offer_id=?1",
                rusqlite::params![offer],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .unwrap();
    assert_eq!(audit, ("paid".into(), "processing".into(), 999));
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn paused_batch_allows_one_single_then_resumes_the_exact_item() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 3, "$60", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    let first = s.active_seller_job(111).unwrap().unwrap().job_ref();
    assert!(s
        .finish_batch_item(batch, 1, &first.token)
        .unwrap()
        .is_some());
    assert!(s.start_batch_item(batch, 2).unwrap());
    let interrupted = s.active_seller_job(111).unwrap().unwrap().job_ref();
    assert!(s
        .set_handoff_state_for_seller_job(
            111,
            &interrupted,
            "gm_ready",
            "http://buyer:proxy@1.2.3.4:8080",
            0,
        )
        .unwrap());

    assert_eq!(s.pause_batch(batch, 111).unwrap(), Some(2));
    assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "paused");
    assert_eq!(
        s.get_batch_item(batch, 2).unwrap().unwrap().status,
        "pending"
    );
    assert!(s.active_seller_job(111).unwrap().is_none());
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "");
    assert!(s
        .finish_batch_item(batch, 2, &interrupted.token)
        .unwrap()
        .is_none());

    let second_batch = s
        .create_batch("Claude Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(!s.accept_batch(second_batch, 111).unwrap());
    let offer = s
        .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
        .unwrap();
    assert!(s.accept_offer(offer, 111, 111).unwrap());
    assert!(s.resume_paused_batch(batch, 111).unwrap().is_none());
    assert!(s.claim_offer_payment(offer, 111).unwrap());
    assert!(s.mark_offer_paid(offer, 111).unwrap());
    let single = s.active_seller_job(111).unwrap().unwrap().job_ref();
    assert!(s.finish_offer_job(111, offer, &single.token).unwrap());

    assert_eq!(s.resume_paused_batch(batch, 111).unwrap(), Some(2));
    let resumed = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(resumed.reference.kind, "batch");
    assert_eq!(resumed.reference.batch_id, batch);
    assert_eq!(resumed.reference.item_no, 2);
    assert_ne!(resumed.reference.token, interrupted.token);
    assert!(s.start_batch_item(batch, 2).unwrap());
    let overview = s.open_batch_overviews(111).unwrap();
    let overview = overview
        .iter()
        .find(|overview| overview.batch.id == batch)
        .unwrap();
    assert_eq!(overview.completed, 1);
    assert_eq!(overview.remaining, 2);
    assert_eq!(s.archive_batch(batch).unwrap(), Some(true));
    assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "cancelled");
    assert_eq!(
        s.get_batch_item(batch, 2).unwrap().unwrap().status,
        "pending"
    );
    assert!(s.active_seller_job(111).unwrap().is_none());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn archiving_a_paused_batch_does_not_clear_the_active_single() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    assert_eq!(s.pause_batch(batch, 111).unwrap(), Some(1));

    let offer = s
        .create_offer_with_proxy("Claude Pro", "$20", 999, 111, "seller", "")
        .unwrap();
    assert!(s.accept_offer(offer, 111, 111).unwrap());
    assert!(s.claim_offer_payment(offer, 111).unwrap());
    assert!(s.mark_offer_paid(offer, 111).unwrap());
    let single = s.active_seller_job(111).unwrap().unwrap().job_ref();
    assert!(s
        .set_handoff_state_for_seller_job(
            111,
            &single,
            "ho_email",
            "http://seller:proxy@1.2.3.4:8080",
            0,
        )
        .unwrap());

    assert_eq!(s.archive_batch(batch).unwrap(), Some(false));
    assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "cancelled");
    let still_single = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(still_single.reference.kind, "offer");
    assert_eq!(still_single.reference.offer_id, offer);
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "ho_email");
    assert!(s.open_batch_overviews(111).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn paused_batch_survives_restart_without_relocking_the_seller() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let batch;
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        batch = s
            .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());
        assert_eq!(s.pause_batch(batch, 111).unwrap(), Some(1));
    }
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_seller_jobs().unwrap(), 0);
    assert!(s.active_seller_job(111).unwrap().is_none());
    assert_eq!(s.get_batch(batch).unwrap().unwrap().status, "paused");
    assert_eq!(s.resume_paused_batch(batch, 111).unwrap(), Some(1));
    assert!(s.start_batch_item(batch, 1).unwrap());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Откат обязан быть атомарным и одноразовым: он двигает шаг, гасит незавершённую
/// PKCE-транзакцию и выдаёт новое поколение, после чего любая capability, захваченная до него,
/// пишет с устаревшим токеном и молча проваливается.
#[test]
fn rewind_handoff_step_moves_one_step_and_rotates_the_generation() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xseller").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    let stale = s.active_seller_job(111).unwrap().unwrap().reference;
    assert!(s
        .set_handoff_state_for_seller_job(111, &stale, "gm_ready", "http://u:p@1.1.1.1:8000", 0)
        .unwrap());
    s.start_gemini_oauth(111, "pending-state", "sealed", now() + 60, 0)
        .unwrap();
    // `start_gemini_oauth` сам переводит продавца в `gm_wait` и выдаёт новое поколение.
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_wait");
    let live = s.active_seller_job(111).unwrap().unwrap().reference;

    let fresh = s
        .rewind_handoff_step(111, &live, "gm_wait", "gm_gproxy", Some(("", 0)))
        .unwrap()
        .expect("шаг назад выполнен");
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_gproxy");
    assert_eq!(s.get_user(111).unwrap().unwrap().hproxy, "");
    assert_ne!(fresh.token, live.token);
    // Незавершённая PKCE-транзакция погашена внутри той же транзакции.
    assert!(s.claim_gemini_oauth("pending-state").unwrap().is_none());
    // Захваченное до отката поколение больше ничего записать не может.
    assert!(!s.set_want_for_seller_job(111, &live, "gm_wait").unwrap());
    assert!(!s
        .set_handoff_state_for_seller_job(111, &live, "gm_ready", "http://x:y@2.2.2.2:9000", 0)
        .unwrap());
    assert!(s.set_want_for_seller_job(111, &fresh, "gm_gproxy").unwrap());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Апдейты диспатчатся конкурентно, поэтому двойное нажатие кнопки — настоящая гонка. Предикат
/// исходного шага живёт в том же statement, что и generation guard, поэтому второй вызов
/// обязан ничего не сделать, а не увести продавца ещё на шаг назад.
#[test]
fn rewind_handoff_step_refuses_a_stale_step_so_a_double_press_is_idempotent() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xseller").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    let live = s.active_seller_job(111).unwrap().unwrap().reference;
    assert!(s
        .set_handoff_state_for_seller_job(111, &live, "gm_ready", "http://u:p@1.1.1.1:8000", 0)
        .unwrap());
    let live = s.active_seller_job(111).unwrap().unwrap().reference;

    let fresh = s
        .rewind_handoff_step(111, &live, "gm_ready", "gm_gproxy", Some(("", 0)))
        .unwrap()
        .expect("первый шаг назад");
    // Повтор с актуальным поколением, но с уже пройденным исходным шагом — отказ.
    assert!(s
        .rewind_handoff_step(111, &fresh, "gm_ready", "gm_gproxy", Some(("", 0)))
        .unwrap()
        .is_none());
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_gproxy");
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// `hproxy_order` — единственная ручка на оплаченный 30-дневный IPRoyal lease. Ни один путь
/// отката не имеет права записать туда ноль: прокси станет сиротой, а деньги уже уплачены.
#[test]
fn rewind_handoff_step_preserves_the_iproyal_order_id() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xseller").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    let live = s.active_seller_job(111).unwrap().unwrap().reference;
    assert!(s
        .set_handoff_state_for_seller_job(111, &live, "gm_ready", "http://u:p@1.1.1.1:8000", 4242)
        .unwrap());
    let live = s.active_seller_job(111).unwrap().unwrap().reference;

    assert!(s
        .rewind_handoff_step(111, &live, "gm_ready", "gm_gproxy", Some(("", 4242)))
        .unwrap()
        .is_some());
    let user = s.get_user(111).unwrap().unwrap();
    assert_eq!(user.hproxy, "");
    assert_eq!(user.hproxy_order, 4242);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Переход внутри одной egress не имеет права трогать закреплённый прокси: его перезапишет
/// только шаг ввода прокси, до которого этот откат не доходит.
#[test]
fn rewind_handoff_step_keeps_the_pinned_proxy_when_it_only_moves_the_step() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch(
            "Claude Max20x",
            "$20",
            2,
            "$40",
            999,
            111,
            "buyer",
            &[
                "http://u:p@1.1.1.1:8000".to_string(),
                "http://u:p@1.1.1.2:8000".to_string(),
            ],
        )
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xbuyer").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    let live = s.active_seller_job(111).unwrap().unwrap().reference;
    assert!(s
        .set_handoff_state_for_seller_job(111, &live, "ho_code", "http://u:p@1.1.1.1:8000", 77)
        .unwrap());
    let live = s.active_seller_job(111).unwrap().unwrap().reference;

    assert!(s
        .rewind_handoff_step(111, &live, "ho_code", "ho_email", None)
        .unwrap()
        .is_some());
    let user = s.get_user(111).unwrap().unwrap();
    assert_eq!(user.want, "ho_email");
    assert_eq!(user.hproxy, "http://u:p@1.1.1.1:8000");
    assert_eq!(user.hproxy_order, 77);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Работа в неопределённой фазе `paying` неизменяема до admin review — откат не исключение.
#[test]
fn rewind_handoff_step_refuses_a_job_that_is_not_processing() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    let accepted = s.active_seller_job(111).unwrap().unwrap();
    assert_ne!(accepted.phase, "processing");
    s.set_want(111, "gm_ready").unwrap();
    assert!(s
        .rewind_handoff_step(
            111,
            &accepted.reference,
            "gm_ready",
            "gm_gproxy",
            Some(("", 0))
        )
        .unwrap()
        .is_none());
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_ready");

    assert!(s.claim_batch_payment(batch).unwrap());
    let paying = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(paying.phase, "paying");
    // Переход в `paying` сбрасывает состояние продавца; ставим шаг заново, чтобы проверять
    // именно отказ по фазе, а не отсутствие исходного шага.
    s.set_want(111, "gm_ready").unwrap();
    assert!(s
        .rewind_handoff_step(
            111,
            &paying.reference,
            "gm_ready",
            "gm_gproxy",
            Some(("", 0))
        )
        .unwrap()
        .is_none());
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_ready");
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

/// Как только callback забрал одноразовый код, откатывать поздно: читатель обязан замолчать,
/// чтобы шаг назад не устраивал гонку с обменом кода.
#[test]
fn pending_gemini_session_returns_only_unclaimed_sessions() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    assert!(s.pending_gemini_session(111).unwrap().is_none());
    s.start_gemini_oauth(111, "pending-state", "sealed", now() + 60, 0)
        .unwrap();
    let session = s.pending_gemini_session(111).unwrap().expect("сессия ждёт");
    assert_eq!(session.state, "pending-state");
    assert_eq!(session.sealed_payload, "sealed");
    assert!(!s.gemini_oauth_in_flight(111).unwrap());
    assert!(s.claim_gemini_oauth("pending-state").unwrap().is_some());
    assert!(s.pending_gemini_session(111).unwrap().is_none());
    // Заклеймленная сессия видна отдельно: откат обязан отказать, а не молча деградировать.
    assert!(s.gemini_oauth_in_flight(111).unwrap());
    let claimed = s
        .active_gemini_session(111)
        .unwrap()
        .expect("/cancel видит exact processing generation");
    assert_eq!(claimed.state, "pending-state");
    assert_eq!(s.interrupted_gemini_chats().unwrap(), vec![111]);

    // Истёкшая сессия тоже не годится: её код всё равно уже не обменять.
    s.start_gemini_oauth(222, "expired-state", "sealed", now() - 1, 0)
        .unwrap();
    assert!(s.pending_gemini_session(222).unwrap().is_none());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn admin_can_rewind_only_the_previous_batch_position() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let offer = s
        .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
        .unwrap();
    let batch = s
        .create_batch("Google AI Pro", "$20", 3, "$60", 999, 111, "seller", &[])
        .unwrap();
    assert!(s.accept_batch(batch, 111).unwrap());
    assert!(s.claim_batch_payment(batch).unwrap());
    assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
    assert!(s.start_batch_item(batch, 1).unwrap());
    let first_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
    assert!(s
        .finish_batch_item(batch, 2, &first_token)
        .unwrap()
        .is_none());
    assert!(s
        .finish_batch_item(batch, 1, &first_token)
        .unwrap()
        .is_some());
    let queued = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(queued.reference.kind, "batch");
    assert_eq!(queued.reference.batch_id, batch);
    assert_eq!(queued.reference.item_no, 2);
    assert_ne!(queued.reference.token, first_token);
    assert!(!s.accept_offer(offer, 111, 111).unwrap());
    assert!(s.start_batch_item(batch, 2).unwrap());

    assert_eq!(s.rewind_batch_to_previous(batch, 111).unwrap(), Some(1));
    assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
    assert_eq!(
        s.get_batch_item(batch, 1).unwrap().unwrap().status,
        "pending"
    );
    assert_eq!(
        s.get_batch_item(batch, 2).unwrap().unwrap().status,
        "pending"
    );
    let rewound = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(rewound.reference.kind, "batch");
    assert_eq!(rewound.reference.batch_id, batch);
    assert_eq!(rewound.reference.item_no, 1);
    assert_ne!(first_token, rewound.reference.token);
    assert!(s.start_batch_item(batch, 1).unwrap());
    let rewound_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
    assert_ne!(first_token, rewound_token);
    let rewound_job = s.active_seller_job(111).unwrap().unwrap().job_ref();
    assert!(s
        .set_handoff_state_for_seller_job(
            111,
            &rewound_job,
            "gm_ready",
            "http://new:proxy@1.2.3.4:8080",
            0,
        )
        .unwrap());
    let mut stale_job = rewound_job.clone();
    stale_job.token = first_token.clone();
    assert!(!s
        .set_want_for_seller_job(111, &stale_job, "cx_email")
        .unwrap());
    assert_eq!(s.get_user(111).unwrap().unwrap().want, "gm_ready");
    assert!(s
        .finish_batch_item(batch, 1, &first_token)
        .unwrap()
        .is_none());
    assert_eq!(s.get_batch(batch).unwrap().unwrap().current_item, 1);
    assert!(s.rewind_batch_to_previous(batch, 111).unwrap().is_none());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn rollout_recovers_the_exact_inflight_batch_position() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let batch;
    {
        let s = Store::open(&p).unwrap();
        batch = s
            .create_batch("Google AI Pro", "$20", 5, "$100", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(batch, 111).unwrap());
        assert!(s.claim_batch_payment(batch).unwrap());
        assert!(s.mark_batch_paid(batch, "0xbatch").unwrap());
        assert!(s.start_batch_item(batch, 1).unwrap());
        let first_token = s.active_seller_job(111).unwrap().unwrap().reference.token;
        assert!(s
            .finish_batch_item(batch, 1, &first_token)
            .unwrap()
            .is_some());
        assert!(s.start_batch_item(batch, 2).unwrap());
        // Simulate the pre-seller_jobs production schema while preserving batch progress.
        s.c.lock()
            .unwrap()
            .execute("DELETE FROM seller_jobs", [])
            .unwrap();
    }
    let s = Store::open(&p).unwrap();
    assert_eq!(s.recover_seller_jobs().unwrap(), 1);
    let job = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(job.reference.kind, "batch");
    assert_eq!(job.reference.batch_id, batch);
    assert_eq!(job.reference.item_no, 2);
    assert_eq!(job.total, 5);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn uncertain_single_payment_stays_locked_until_admin_review() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.register_user(111, 111, "seller").unwrap();
        let offer = s
            .create_offer_with_proxy("ChatGPT Plus", "$20", 999, 111, "seller", "")
            .unwrap();
        s.set_response(offer, 111, "accepted").unwrap();
        assert!(s.claim_offer_payment(offer, 111).unwrap());
    }
    let s = Store::open(&p).unwrap();
    let job = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(job.reference.kind, "offer");
    assert_eq!(job.phase, "paying");
    assert!(!s.claim_offer_payment(job.reference.offer_id, 111).unwrap());
    assert!(s.reset_offer_payment(job.reference.offer_id, 111).unwrap());
    let accepted = s.active_seller_job(111).unwrap().unwrap();
    assert_eq!(accepted.reference.offer_id, job.reference.offer_id);
    assert_eq!(accepted.phase, "accepted");
    assert_eq!(
        s.response_status(job.reference.offer_id, 111)
            .unwrap()
            .as_deref(),
        Some("accepted")
    );
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn gemini_oauth_session_keeps_the_exact_seller_job() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    let s = Store::open(&p).unwrap();
    s.register_user(111, 111, "seller").unwrap();
    let offer = s
        .create_offer_with_proxy("Google AI Pro", "$20", 999, 111, "seller", "")
        .unwrap();
    s.set_response(offer, 111, "accepted").unwrap();
    assert!(s.claim_offer_payment(offer, 111).unwrap());
    assert!(s.mark_offer_paid(offer, 111).unwrap());
    s.start_gemini_oauth(111, "bound-state", "sealed", now() + 60, 0)
        .unwrap();
    let expected_job = s.active_seller_job(111).unwrap().unwrap().job_ref();
    let session = s.claim_gemini_oauth("bound-state").unwrap().unwrap();
    assert_eq!(session.job, Some(expected_job.clone()));
    let final_job = s
        .advance_gemini_oauth(&session, "final-state", "final-sealed", now() + 60, 77)
        .unwrap()
        .unwrap();
    assert_ne!(final_job.token, expected_job.token);
    assert!(s.claim_gemini_oauth("bound-state").unwrap().is_none());
    let final_session = s
        .pending_gemini_session_by_state("final-state")
        .unwrap()
        .unwrap();
    assert_eq!(final_session.job, Some(final_job.clone()));
    assert_eq!(s.get_user(111).unwrap().unwrap().hproxy_order, 77);
    assert!(s
        .advance_gemini_oauth(&session, "stale-final-state", "stale-sealed", now() + 60, 0,)
        .is_err());
    // Шаг назад — единственный механизм возврата: он же ротирует поколение, после чего
    // повтор с тем же поколением и завершение работы по нему обязаны провалиться.
    assert!(s
        .rewind_handoff_step(111, &final_job, "gm_wait", "gm_gproxy", Some(("", 77)))
        .unwrap()
        .is_some());
    let retry_job = s.active_seller_job(111).unwrap().unwrap().job_ref();
    assert_ne!(retry_job.token, final_job.token);
    assert!(s
        .rewind_handoff_step(111, &final_job, "gm_wait", "gm_gproxy", Some(("", 77)))
        .unwrap()
        .is_none());
    assert!(!s.finish_offer_job(111, offer, &expected_job.token).unwrap());
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn uncertain_batch_payment_stays_locked_across_restart() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        let id = s
            .create_batch("Google AI Pro", "$20", 2, "$40", 999, 111, "seller", &[])
            .unwrap();
        assert!(s.accept_batch(id, 111).unwrap());
        assert!(s.claim_batch_payment(id).unwrap());
    }
    let s = Store::open(&p).unwrap();
    let review = s.batches_needing_payment_review().unwrap();
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].status, "paying");
    assert!(s.archive_batch(review[0].id).unwrap().is_none());
    assert!(s.reset_batch_payment(review[0].id).unwrap());
    assert_eq!(
        s.get_batch(review[0].id).unwrap().unwrap().status,
        "accepted"
    );
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}

#[test]
fn admin_batch_draft_survives_restart_without_losing_proxy_order() {
    let p = tmp();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
    {
        let s = Store::open(&p).unwrap();
        s.set_admin_flow(&AdminState {
            chat_id: 999,
            step: "batch_proxies".into(),
            product: "Claude Pro".into(),
            seller_chat: 111,
            mode: "batch".into(),
            quantity: 2,
            unit_price: "$20".into(),
            proxy_source: "buyer".into(),
            draft_proxies: vec!["http://u:p@1.1.1.1:80".into()],
        })
        .unwrap();
    }
    let s = Store::open(&p).unwrap();
    let state = s.get_admin_flow(999).unwrap().unwrap();
    assert_eq!(state.mode, "batch");
    assert_eq!(state.step, "batch_proxies");
    assert_eq!(state.draft_proxies, vec!["http://u:p@1.1.1.1:80"]);
    let _ = std::fs::remove_dir_all(std::path::Path::new(&p).parent().unwrap());
}
