-- Expand the durable cursor namespace before the topups-v2 consumer is deployed. The replacement
-- check is installed and validated before the legacy check is removed, so every pre-existing row
-- remains continuously constrained and old Sales binaries keep using their unchanged cursor keys.
ALTER TABLE "sync_cursors" ADD CONSTRAINT "sync_cursors_feed_v2_check"
  CHECK ("feed" IN ('attributions', 'usage_events', 'topups', 'topups_v2')) NOT VALID;--> statement-breakpoint
ALTER TABLE "sync_cursors" VALIDATE CONSTRAINT "sync_cursors_feed_v2_check";--> statement-breakpoint
ALTER TABLE "sync_cursors" DROP CONSTRAINT "sync_cursors_feed_check";
