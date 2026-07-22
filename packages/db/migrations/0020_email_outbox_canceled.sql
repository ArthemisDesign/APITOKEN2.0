-- PostgreSQL cannot use a newly appended enum value until the transaction that added it commits,
-- while Drizzle intentionally applies the complete pending migration set in one transaction.
-- Rebuild this small enum transactionally so the data correction below is atomic with the schema.
LOCK TABLE "email_outbox" IN ACCESS EXCLUSIVE MODE;--> statement-breakpoint
ALTER TABLE "email_outbox" ALTER COLUMN "status" DROP DEFAULT;--> statement-breakpoint
ALTER TYPE "email_outbox_status" RENAME TO "email_outbox_status_old";--> statement-breakpoint
CREATE TYPE "email_outbox_status" AS ENUM ('pending', 'processing', 'sent', 'failed', 'canceled');--> statement-breakpoint
ALTER TABLE "email_outbox" ALTER COLUMN "status" TYPE "email_outbox_status"
  USING "status"::text::"email_outbox_status";--> statement-breakpoint
DROP TYPE "email_outbox_status_old";--> statement-breakpoint
ALTER TABLE "email_outbox" ALTER COLUMN "status" SET DEFAULT 'pending'::"email_outbox_status";--> statement-breakpoint

-- These are the exact zero-attempt verification jobs intentionally retired when verification was
-- disabled. Provider failures, exhausted retries, malformed payloads, and any future error remain
-- in the genuine failed state and continue to page operators.
UPDATE "email_outbox"
SET "status" = 'canceled', "updated_at" = now()
WHERE "status" = 'failed'
  AND "template" = 'verify_email'
  AND "attempts" = 0
  AND "provider_message_id" IS NULL
  AND "sent_at" IS NULL
  AND "locked_at" IS NULL
  AND "locked_by" IS NULL
  AND "last_error" = 'verification delivery canceled while email verification is disabled';
