ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_face_value_positive" CHECK ("face_value_nano" > 0) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_mult_bp_range" CHECK ("mult_bp" BETWEEN 1 AND 10000) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_quantity_range" CHECK ("quantity" BETWEEN 1 AND 100) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_label_length" CHECK ("label" IS NULL OR char_length("label") <= 200) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_note_length" CHECK ("note" IS NULL OR char_length("note") <= 2000) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_created_by_length" CHECK (char_length("created_by") BETWEEN 1 AND 128) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_face_value_positive" CHECK ("face_value_nano" > 0) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_mult_bp_range" CHECK ("mult_bp" BETWEEN 1 AND 10000) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_view_token_shape" CHECK ("view_token" ~ '^[A-Za-z0-9_-]{22}$') NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_secret_pair" CHECK (("secret_ciphertext" IS NULL) = ("secret_nonce" IS NULL)) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_delivered_secret_cleared" CHECK ("delivered_at" IS NULL OR ("secret_ciphertext" IS NULL AND "secret_nonce" IS NULL)) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_disabled_timestamp" CHECK ("status" <> 'disabled' OR "disabled_at" IS NOT NULL) NOT VALID;
