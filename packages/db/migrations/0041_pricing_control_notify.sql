-- Wake the commerce pricing worker the moment a pricing-control job becomes durable.
-- pg_notify is delivered on COMMIT of the inserting transaction, so a notification can
-- never reference a job that rolled back. The periodic worker sweep stays as the
-- recovery path for notifications missed while a listener was down.
CREATE FUNCTION "notify_pricing_control_job"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  PERFORM pg_notify('pricing_control_jobs', TG_TABLE_NAME);
  RETURN NULL;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "engine_catalog_jobs_notify"
AFTER INSERT ON "engine_catalog_jobs"
FOR EACH ROW EXECUTE FUNCTION "notify_pricing_control_job"();--> statement-breakpoint

CREATE TRIGGER "engine_switch_jobs_notify"
AFTER INSERT ON "engine_switch_jobs"
FOR EACH ROW EXECUTE FUNCTION "notify_pricing_control_job"();--> statement-breakpoint

CREATE TRIGGER "engine_policy_jobs_notify"
AFTER INSERT ON "engine_policy_jobs"
FOR EACH ROW EXECUTE FUNCTION "notify_pricing_control_job"();
