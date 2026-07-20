-- Буфер спенд/депозит-событий, пришедших раньше атрибуции пользователя (D1). Reconcile проигрывает
-- их идемпотентно, как только юзер появляется в referred_users, и удаляет строку.
CREATE TABLE "pending_referral_events" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"kind" text NOT NULL,
	"commerce_ref" text NOT NULL,
	"commerce_user_id" uuid NOT NULL,
	"amount_nano" bigint NOT NULL,
	"occurred_at" timestamp with time zone NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX "pending_referral_events_kind_ref_uidx" ON "pending_referral_events" USING btree ("kind","commerce_ref");--> statement-breakpoint
CREATE INDEX "pending_referral_events_user_idx" ON "pending_referral_events" USING btree ("commerce_user_id");
