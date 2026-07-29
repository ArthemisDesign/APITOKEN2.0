-- Expand-only product discriminator. Existing rows deliberately remain NULL and
-- are interpreted as Anthropic by both old and new application releases.
ALTER TABLE "openkeys_batches" ADD COLUMN "api_type" text;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_api_type" CHECK ("api_type" IS NULL OR "api_type" IN ('anthropic', 'openai')) NOT VALID;
