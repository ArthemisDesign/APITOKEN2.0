-- Release the retired pricing design's hold on the engine owner lease.
--
-- Two triggers gate who may claim or heartbeat an owner epoch, and both are conditional on the
-- retired design still being present in the database:
--
--   * `engine_instances_policy_runtime_floor` (migration 0017) demands pricing runtime manifest
--     pins while any account binding is strict;
--   * `engine_instances_release_v2_epoch_fence` (migration 0025) demands a release runtime digest
--     bound to the owner epoch while a release head row exists.
--
-- Both protected a rollback to a binary that could not honour the newer pricing authority. That
-- authority is gone — a request is priced by the account's discount — so no binary publishes
-- either set of pins any more. Production still carries one strict binding and an activated
-- release head, so the first trigger refused the blue-green cutover outright
-- (`strict pricing requires a policy-capable engine runtime manifest`) and the watchdog rolled
-- the release back; the second would have refused the very next attempt for the same reason.
--
-- The columns stay. All three shape CHECKs accept the all-NULL row a current binary writes, and
-- rows an older peer already wrote remain valid — dropping a check never invalidates data that
-- already satisfied it, so a draining blue-green peer keeps working until it retires.

DROP TRIGGER IF EXISTS engine_instances_policy_runtime_floor ON engine_instances;
DROP FUNCTION IF EXISTS enforce_policy_capable_engine_instance();

DROP TRIGGER IF EXISTS engine_instances_release_v2_epoch_fence ON engine_instances;
DROP FUNCTION IF EXISTS enforce_pricing_release_runtime_epoch_v2();

INSERT INTO engine_schema_migrations(version) VALUES (45)
ON CONFLICT (version) DO NOTHING;
