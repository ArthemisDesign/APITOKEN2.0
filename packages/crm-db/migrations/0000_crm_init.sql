CREATE TYPE "public"."attribute_status" AS ENUM('proposed', 'active', 'merged');--> statement-breakpoint
CREATE TYPE "public"."contact_status" AS ENUM('new', 'enriched', 'qualified', 'archived');--> statement-breakpoint
CREATE TYPE "public"."view_creator" AS ENUM('ai', 'human');--> statement-breakpoint
CREATE TABLE "ai_audit" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"kind" text NOT NULL,
	"model" text NOT NULL,
	"input_summary" text,
	"output" jsonb,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "attribute_registry" (
	"key" text PRIMARY KEY NOT NULL,
	"description" text,
	"value_kind" text,
	"examples" jsonb DEFAULT '[]'::jsonb NOT NULL,
	"status" "attribute_status" DEFAULT 'proposed' NOT NULL,
	"merged_into" text,
	"ai_notes" text,
	"seen_count" integer DEFAULT 0 NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "contact_attributes" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"contact_id" uuid NOT NULL,
	"key" text NOT NULL,
	"value" jsonb NOT NULL,
	"confidence" real,
	"evidence" text,
	"source" text,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "contact_channels" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"contact_id" uuid NOT NULL,
	"type" text NOT NULL,
	"value" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "contacts" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"name" text,
	"summary" text,
	"status" "contact_status" DEFAULT 'new' NOT NULL,
	"last_parser" text,
	"last_run_id" text,
	"hypotheses" jsonb DEFAULT '[]'::jsonb NOT NULL,
	"raw" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "ingest_runs" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"run_id" text NOT NULL,
	"parser" text NOT NULL,
	"received" integer DEFAULT 0 NOT NULL,
	"accepted" integer DEFAULT 0 NOT NULL,
	"repaired" integer DEFAULT 0 NOT NULL,
	"rejected" integer DEFAULT 0 NOT NULL,
	"drift" jsonb DEFAULT '[]'::jsonb NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "smart_views" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"title" text NOT NULL,
	"description" text,
	"filter" jsonb NOT NULL,
	"rationale" text,
	"created_by" "view_creator" DEFAULT 'ai' NOT NULL,
	"contact_count" integer DEFAULT 0 NOT NULL,
	"refreshed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "contact_attributes" ADD CONSTRAINT "contact_attributes_contact_id_contacts_id_fk" FOREIGN KEY ("contact_id") REFERENCES "public"."contacts"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "contact_channels" ADD CONSTRAINT "contact_channels_contact_id_contacts_id_fk" FOREIGN KEY ("contact_id") REFERENCES "public"."contacts"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "ai_audit_kind_idx" ON "ai_audit" USING btree ("kind");--> statement-breakpoint
CREATE UNIQUE INDEX "contact_attributes_contact_key_uq" ON "contact_attributes" USING btree ("contact_id","key");--> statement-breakpoint
CREATE INDEX "contact_attributes_key_idx" ON "contact_attributes" USING btree ("key");--> statement-breakpoint
CREATE UNIQUE INDEX "contact_channels_type_value_uq" ON "contact_channels" USING btree ("type","value");--> statement-breakpoint
CREATE INDEX "contact_channels_contact_idx" ON "contact_channels" USING btree ("contact_id");--> statement-breakpoint
CREATE INDEX "ingest_runs_run_idx" ON "ingest_runs" USING btree ("run_id");