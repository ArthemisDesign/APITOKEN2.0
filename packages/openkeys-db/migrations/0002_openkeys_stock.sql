ALTER TABLE "openkeys_keys" ADD COLUMN "secret_ciphertext" text;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "secret_nonce" text;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "delivered_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "removed_at" timestamp with time zone;
