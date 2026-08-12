-- Commit-bound invalidation feed for central OpenKeys administration. Statement-level triggers
-- keep batch issuance O(1) for notification fanout while covering writes from every process.
CREATE OR REPLACE FUNCTION notify_openkeys_admin_change()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	PERFORM pg_notify('openkeys_admin_changes', TG_TABLE_NAME);
	RETURN NULL;
END;
$$;
--> statement-breakpoint

CREATE TRIGGER openkeys_batches_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON openkeys_batches
	FOR EACH STATEMENT EXECUTE FUNCTION notify_openkeys_admin_change();
CREATE TRIGGER openkeys_keys_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON openkeys_keys
	FOR EACH STATEMENT EXECUTE FUNCTION notify_openkeys_admin_change();
CREATE TRIGGER openkeys_issuance_jobs_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON openkeys_issuance_jobs
	FOR EACH STATEMENT EXECUTE FUNCTION notify_openkeys_admin_change();
