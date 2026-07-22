-- Anti-abuse Layer 1: per-user signup profile (IP/subnet/UA/device fingerprint) with a
-- one-bonus-per-cluster guarantee enforced by partial unique indexes, plus a device→user
-- sighting log that links every account a browser ever signs into.
CREATE TABLE "signup_profiles" (
  "user_id" uuid PRIMARY KEY REFERENCES "users"("id") ON DELETE CASCADE,
  "email_canonical" text NOT NULL,
  "ip_address" text,
  "ip_subnet" text,
  "user_agent" text,
  "device_hash" text,
  "bonus_granted" boolean NOT NULL DEFAULT false,
  "flagged_reason" text,
  "created_at" timestamp with time zone NOT NULL DEFAULT now()
);--> statement-breakpoint
-- One welcome bonus per device cookie, per /24 (or v6 /64) subnet, per canonical email — a
-- second claim from the same cluster hits the index and is denied atomically (race-proof).
CREATE UNIQUE INDEX "signup_bonus_device_uidx" ON "signup_profiles" ("device_hash") WHERE "bonus_granted";--> statement-breakpoint
CREATE UNIQUE INDEX "signup_bonus_subnet_uidx" ON "signup_profiles" ("ip_subnet") WHERE "bonus_granted";--> statement-breakpoint
CREATE UNIQUE INDEX "signup_bonus_email_uidx" ON "signup_profiles" ("email_canonical") WHERE "bonus_granted";--> statement-breakpoint
CREATE INDEX "signup_profiles_subnet_idx" ON "signup_profiles" ("ip_subnet", "created_at");--> statement-breakpoint
CREATE TABLE "device_sightings" (
  "device_hash" text NOT NULL,
  "user_id" uuid NOT NULL REFERENCES "users"("id") ON DELETE CASCADE,
  "first_seen_at" timestamp with time zone NOT NULL DEFAULT now(),
  "last_seen_at" timestamp with time zone NOT NULL DEFAULT now(),
  PRIMARY KEY ("device_hash", "user_id")
);--> statement-breakpoint
CREATE INDEX "device_sightings_user_idx" ON "device_sightings" ("user_id");--> statement-breakpoint

-- Backfill: every existing user gets a profile keyed by canonical email (gmail dots/+aliases
-- collapse). Existing OAuth users are recorded as bonus_granted so the same canonical email,
-- and any future device/subnet they surface from, can never claim a second bonus. When several
-- existing accounts collapse to one canonical email, only the earliest keeps the granted slot
-- (the partial unique index requires it); later ones are flagged for review instead.
WITH ranked AS (
  SELECT
    u.id,
    u.created_at,
    lower(
      CASE
        WHEN lower(split_part(u.email, '@', 2)) IN ('gmail.com', 'googlemail.com')
          THEN replace(split_part(split_part(u.email, '@', 1), '+', 1), '.', '') || '@gmail.com'
        ELSE split_part(split_part(u.email, '@', 1), '+', 1) || '@' || split_part(u.email, '@', 2)
      END
    ) AS canon,
    EXISTS (
      SELECT 1 FROM auth_identities i
      WHERE i.user_id = u.id AND i.provider IN ('google', 'github')
    ) AS has_oauth
  FROM users u
),
ordered AS (
  SELECT *,
    row_number() OVER (PARTITION BY canon ORDER BY has_oauth DESC, created_at) AS rn
  FROM ranked
)
INSERT INTO signup_profiles (user_id, email_canonical, bonus_granted, flagged_reason, created_at)
SELECT
  id,
  canon,
  has_oauth AND rn = 1,
  CASE WHEN has_oauth AND rn > 1 THEN 'backfill-duplicate-email' END,
  created_at
FROM ordered
ON CONFLICT (user_id) DO NOTHING;
