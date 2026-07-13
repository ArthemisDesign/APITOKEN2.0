CREATE TYPE "public"."oauth_provider" AS ENUM('google', 'github');--> statement-breakpoint
CREATE TABLE "oauth_transactions" (
	"state_hash" text PRIMARY KEY NOT NULL,
	"provider" "oauth_provider" NOT NULL,
	"nonce" text,
	"code_verifier" text NOT NULL,
	"invite_token_hash" text,
	"expires_at" timestamp with time zone NOT NULL,
	"consumed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "email_outbox" ADD COLUMN "locked_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "email_outbox" ADD COLUMN "locked_by" text;--> statement-breakpoint
ALTER TABLE "email_outbox" ADD COLUMN "provider_message_id" text;--> statement-breakpoint
ALTER TABLE "email_outbox" ADD COLUMN "updated_at" timestamp with time zone DEFAULT now() NOT NULL;--> statement-breakpoint
CREATE INDEX "oauth_transactions_expiry_idx" ON "oauth_transactions" USING btree ("expires_at");--> statement-breakpoint
UPDATE "email_outbox"
SET "status" = 'failed', "last_error" = 'legacy email job has no encrypted token', "updated_at" = now()
WHERE "status" = 'pending' AND NOT ("payload" ? 'encryptedToken');
