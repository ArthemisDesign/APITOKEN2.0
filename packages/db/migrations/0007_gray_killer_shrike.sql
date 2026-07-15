ALTER TABLE "users" ADD COLUMN "display_name" text;--> statement-breakpoint
UPDATE "users" AS u
SET "display_name" = COALESCE(
  (
    SELECT NULLIF(LEFT(BTRIM(ai."metadata" ->> 'displayName'), 80), '')
    FROM "auth_identities" AS ai
    WHERE ai."user_id" = u."id"
    ORDER BY ai."created_at" ASC
    LIMIT 1
  ),
  NULLIF(LEFT(SPLIT_PART(u."email", '@', 1), 80), ''),
  'User'
);--> statement-breakpoint
ALTER TABLE "users" ALTER COLUMN "display_name" SET NOT NULL;
