-- Transactional invalidation feed for the managed admin console. PostgreSQL delivers NOTIFY only
-- after COMMIT, so listeners never refresh for rolled-back writes. The stable table-name payload
-- lets the separately delivered API consumer invalidate only affected admin resources; identical
-- notifications inside one transaction are coalesced by PostgreSQL.
CREATE OR REPLACE FUNCTION notify_commerce_admin_change()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	PERFORM pg_notify('commerce_admin_changes', TG_TABLE_NAME);
	RETURN NULL;
END;
$$;
--> statement-breakpoint

CREATE TRIGGER users_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON users
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER customer_profiles_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON customer_profiles
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER business_invites_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON business_invites
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER signup_profiles_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON signup_profiles
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER engine_accounts_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON engine_accounts
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER engine_pricing_jobs_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON engine_pricing_jobs
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER customer_provider_discounts_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON customer_provider_discounts
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER payments_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON payments
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER checkout_sessions_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON checkout_sessions
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER engine_credits_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON engine_credits
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER engine_adjustments_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON engine_adjustments
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER webhook_events_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON webhook_events
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER email_outbox_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON email_outbox
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER pricing_usage_events_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON pricing_usage_events
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER pricing_usage_topups_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON pricing_usage_topups
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER pricing_usage_attributions_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON pricing_usage_attributions
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER api_keys_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON api_keys
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER audit_log_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON audit_log
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER admin_accounts_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON admin_accounts
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
CREATE TRIGGER admin_account_domains_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON admin_account_domains
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
