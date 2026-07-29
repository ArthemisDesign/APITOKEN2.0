-- Existing batches and keys keep their exact multiplier and economics. The temporary legacy
-- default keeps the current writer and rollback release compatible; the 1:1 writer and final
-- default cutover ship only after this migration is confirmed in production.
ALTER TABLE "openkeys_batches"
	ADD COLUMN "pricing_contract" text DEFAULT 'legacy' NOT NULL;
--> statement-breakpoint
ALTER TABLE "openkeys_keys"
	ADD COLUMN "pricing_contract" text DEFAULT 'legacy' NOT NULL;
--> statement-breakpoint

-- NOT VALID keeps the expand transaction short on an unknown live table size. PostgreSQL still
-- enforces these constraints for every new INSERT/UPDATE; historical rows are validated after
-- the production inventory, in a separate non-blocking migration.
ALTER TABLE "openkeys_batches"
	ADD CONSTRAINT "openkeys_batches_id_pricing_contract_unique"
	UNIQUE ("id", "pricing_contract");
--> statement-breakpoint
ALTER TABLE "openkeys_keys"
	ADD CONSTRAINT "openkeys_keys_batch_contract_fk"
	FOREIGN KEY ("batch_id", "pricing_contract")
	REFERENCES "public"."openkeys_batches"("id", "pricing_contract")
	ON DELETE restrict ON UPDATE no action NOT VALID;
--> statement-breakpoint

ALTER TABLE "openkeys_batches"
	ADD CONSTRAINT "openkeys_batches_pricing_contract"
	CHECK ("pricing_contract" IN ('legacy', 'official_1_to_1')) NOT VALID;
--> statement-breakpoint
ALTER TABLE "openkeys_batches"
	ADD CONSTRAINT "openkeys_batches_official_1_to_1"
	CHECK ("pricing_contract" <> 'official_1_to_1' OR "mult_bp" = 10000) NOT VALID;
--> statement-breakpoint
ALTER TABLE "openkeys_keys"
	ADD CONSTRAINT "openkeys_keys_pricing_contract"
	CHECK ("pricing_contract" IN ('legacy', 'official_1_to_1')) NOT VALID;
--> statement-breakpoint
ALTER TABLE "openkeys_keys"
	ADD CONSTRAINT "openkeys_keys_official_1_to_1"
	CHECK ("pricing_contract" <> 'official_1_to_1' OR "mult_bp" = 10000) NOT VALID;
