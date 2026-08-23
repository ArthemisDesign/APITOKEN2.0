-- Partner access applications: an ordinary Commerce account asks to join the partner program and
-- an administrator approves or rejects the request. Approval is executed by the existing partner
-- onboarding path, so this table records the review, never the partner terms themselves.
-- The number 0049 is deliberately skipped: `0049_retire_pricing_schema.sql` is the reserved name
-- of the pricing-retirement contraction (packages/db/MIGRATIONS.md, docs/ops/PRICING_RETIREMENT.md).
CREATE TABLE IF NOT EXISTS referral_applications (
	id uuid PRIMARY KEY,
	user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
	status text NOT NULL DEFAULT 'pending',
	message text NOT NULL DEFAULT '',
	reviewer_actor text,
	reviewer_note text,
	decided_at timestamptz,
	created_at timestamptz NOT NULL DEFAULT now(),
	updated_at timestamptz NOT NULL DEFAULT now(),
	CONSTRAINT referral_applications_status_check CHECK (status IN ('pending', 'approved', 'rejected')),
	CONSTRAINT referral_applications_message_check CHECK (length(message) <= 2000),
	CONSTRAINT referral_applications_note_check CHECK (reviewer_note IS NULL OR length(reviewer_note) <= 2000),
	CONSTRAINT referral_applications_decision_check CHECK (
		(status = 'pending' AND decided_at IS NULL AND reviewer_actor IS NULL)
		OR (status <> 'pending' AND decided_at IS NOT NULL AND reviewer_actor IS NOT NULL)
	)
);
--> statement-breakpoint

-- One open application per account: a second submit updates the pending row instead of queueing.
CREATE UNIQUE INDEX IF NOT EXISTS referral_applications_pending_uidx
	ON referral_applications (user_id) WHERE status = 'pending';
--> statement-breakpoint

CREATE INDEX IF NOT EXISTS referral_applications_queue_idx
	ON referral_applications (status, created_at DESC);
--> statement-breakpoint

CREATE TRIGGER referral_applications_admin_change_notify
	AFTER INSERT OR UPDATE OR DELETE ON referral_applications
	FOR EACH STATEMENT EXECUTE FUNCTION notify_commerce_admin_change();
