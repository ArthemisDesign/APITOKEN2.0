-- Commit-bound invalidation feed for the central partner administration view. One statement-level
-- notification covers bulk writes without adding per-row fanout; listeners map the stable table
-- name to the affected read models.
CREATE OR REPLACE FUNCTION notify_sales_admin_change()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
	PERFORM pg_notify('sales_admin_changes', TG_TABLE_NAME);
	RETURN NULL;
END;
$$;
--> statement-breakpoint

CREATE TRIGGER partners_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partners
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER partner_applications_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partner_applications
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER partner_invites_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partner_invites
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER partner_discount_links_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partner_discount_links
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER promo_codes_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON promo_codes
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER referred_users_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON referred_users
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER partner_usage_events_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partner_usage_events
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER partner_usage_events_v2_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partner_usage_events_v2
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER referred_topups_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON referred_topups
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER commission_entries_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON commission_entries
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER commission_entries_v2_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON commission_entries_v2
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER partner_commission_adjustments_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON partner_commission_adjustments
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER payout_batches_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON payout_batches
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER payouts_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON payouts
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
CREATE TRIGGER sales_audit_log_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON sales_audit_log
	FOR EACH STATEMENT EXECUTE FUNCTION notify_sales_admin_change();
