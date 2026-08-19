-- The right to turn one's own referrals into B2B customers, and the ceiling on how deep a
-- discount that partner may give them.
--
-- Default is OFF with a zero ceiling: an ordinary partner keeps doing exactly what partners do
-- today — share a link, the person becomes a normal B2C customer on the global discount, and the
-- partner earns commission on what that customer actually pays. The grant is an explicit,
-- per-partner exception an admin makes, never something a partner acquires by default.
--
-- The ceiling is the safety property. A deeper discount is margin the company gives away, so the
-- maximum lives on the partner row (server side, admin-written) and every issuing path must clamp
-- to it. `b2b_max_discount_bps` is meaningless while `b2b_enabled` is false, and the CHECK keeps
-- that pair honest instead of letting a stale ceiling linger as if it were authority.
--
-- Deliberately NOT reusing referral_discount_bps/referral_discount_enabled: those are the retired
-- marker columns with no price effect, kept only for expand-only replay. Overloading them again
-- would resurrect a semantics the codebase spent a migration removing.
ALTER TABLE "partners" ADD COLUMN "b2b_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "b2b_max_discount_bps" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_b2b_max_discount_check"
	CHECK ("b2b_max_discount_bps" BETWEEN 0 AND 9500
	       AND ("b2b_enabled" OR "b2b_max_discount_bps" = 0));--> statement-breakpoint

-- The same pair on invites, so the capability can be part of onboarding: an invite issued with it
-- creates a partner who already holds the grant, instead of requiring a second admin action after
-- the partner signs in.
ALTER TABLE "partner_invites" ADD COLUMN "b2b_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "b2b_max_discount_bps" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_b2b_max_discount_check"
	CHECK ("b2b_max_discount_bps" BETWEEN 0 AND 9500
	       AND ("b2b_enabled" OR "b2b_max_discount_bps" = 0));
