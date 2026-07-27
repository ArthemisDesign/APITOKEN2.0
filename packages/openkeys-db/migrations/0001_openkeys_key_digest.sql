ALTER TABLE "openkeys_keys" ADD COLUMN "key_sha256" text;--> statement-breakpoint
CREATE UNIQUE INDEX "openkeys_keys_key_sha256_key" ON "openkeys_keys" USING btree ("key_sha256");
