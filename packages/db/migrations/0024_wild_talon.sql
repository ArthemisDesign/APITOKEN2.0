ALTER TABLE "business_invites" ALTER COLUMN "email" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "email_outbox" ALTER COLUMN "user_id" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "business_invites" ADD COLUMN "encrypted_token" text;--> statement-breakpoint
ALTER TABLE "business_invites" ADD COLUMN "revoked_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "business_invites" ADD COLUMN "revoked_by_actor" text;--> statement-breakpoint
ALTER TABLE "business_invites" ADD COLUMN "superseded_by_invite_id" uuid;--> statement-breakpoint
ALTER TABLE "business_invites" ADD COLUMN "idempotency_key" uuid;--> statement-breakpoint
ALTER TABLE "business_invites" ADD COLUMN "created_by_actor" text;--> statement-breakpoint
ALTER TABLE "email_outbox" ADD COLUMN "business_invite_id" uuid;--> statement-breakpoint
ALTER TABLE "business_invites" ADD CONSTRAINT "business_invites_superseded_by_invite_id_fk" FOREIGN KEY ("superseded_by_invite_id") REFERENCES "public"."business_invites"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "email_outbox" ADD CONSTRAINT "email_outbox_business_invite_id_business_invites_id_fk" FOREIGN KEY ("business_invite_id") REFERENCES "public"."business_invites"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "business_invites_idempotency_key_uidx" ON "business_invites" USING btree ("idempotency_key") WHERE "business_invites"."idempotency_key" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "email_outbox_business_invite_idx" ON "email_outbox" USING btree ("business_invite_id","created_at");--> statement-breakpoint
ALTER TABLE "email_outbox" ADD CONSTRAINT "email_outbox_owner_check" CHECK (
    num_nonnulls("email_outbox"."user_id", "email_outbox"."business_invite_id") = 1
  );