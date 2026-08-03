ALTER TABLE "signup_profiles" ADD COLUMN "bonus_amount_nano" bigint;--> statement-breakpoint
-- Every bonus issued by the previously deployed writer had the immutable $4 nominal. Keep the
-- column nullable because that writer remains live during this expand checkpoint and may still
-- claim another row before the amount-aware consumer is deployed.
UPDATE "signup_profiles"
SET "bonus_amount_nano" = 4000000000
WHERE "bonus_granted" AND "bonus_amount_nano" IS NULL;
