-- Opaque per-contact referral aliases for trusted external sales tools. The alias keeps the
-- customer on the ordinary partner attribution/commission path without exposing the external
-- system's identifier in the public URL or overloading legacy one-time discount markers.
CREATE TABLE "external_referral_aliases" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"source" text NOT NULL,
	"external_ref" text NOT NULL,
	"alias_code" text NOT NULL,
	"partner_id" uuid NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "external_referral_aliases_partner_id_partners_id_fk"
		FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id")
		ON DELETE restrict ON UPDATE no action,
	CONSTRAINT "external_referral_aliases_source_check"
		CHECK ("external_referral_aliases"."source" ~ '^[a-z][a-z0-9_-]{1,31}$'),
	CONSTRAINT "external_referral_aliases_external_ref_check"
		CHECK (length("external_referral_aliases"."external_ref") BETWEEN 1 AND 128),
	CONSTRAINT "external_referral_aliases_code_check"
		CHECK ("external_referral_aliases"."alias_code" ~ '^[a-z0-9_-]{3,32}$')
);
--> statement-breakpoint
CREATE UNIQUE INDEX "external_referral_aliases_source_ref_uidx"
	ON "external_referral_aliases" USING btree ("source", "external_ref");
--> statement-breakpoint
CREATE UNIQUE INDEX "external_referral_aliases_code_uidx"
	ON "external_referral_aliases" USING btree ("alias_code");
--> statement-breakpoint
CREATE INDEX "external_referral_aliases_partner_idx"
	ON "external_referral_aliases" USING btree ("partner_id", "created_at");
