CREATE TABLE "admin_account_domains" (
	"admin_account_id" uuid NOT NULL,
	"domain" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "admin_account_domains_admin_account_id_domain_pk" PRIMARY KEY("admin_account_id","domain"),
	CONSTRAINT "admin_account_domains_domain_check" CHECK ("admin_account_domains"."domain" IN (
    'admin.apitoken.sale',
    'admin.partners.apitoken.sale',
    'crm.apitoken.sale',
    'content-studio.apitoken.sale'
  ))
);
--> statement-breakpoint
CREATE TABLE "admin_accounts" (
	"id" uuid PRIMARY KEY NOT NULL,
	"username" text NOT NULL,
	"password_hash" text NOT NULL,
	"status" text DEFAULT 'active' NOT NULL,
	"password_changed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "admin_accounts_username_check" CHECK (
    "admin_accounts"."username" = btrim("admin_accounts"."username")
    AND length("admin_accounts"."username") BETWEEN 1 AND 80
    AND "admin_accounts"."username" ~ '^[A-Za-z0-9._@-]+$'
  ),
	CONSTRAINT "admin_accounts_password_hash_check" CHECK (length("admin_accounts"."password_hash") BETWEEN 20 AND 512),
	CONSTRAINT "admin_accounts_status_check" CHECK ("admin_accounts"."status" IN ('active', 'disabled'))
);
--> statement-breakpoint
ALTER TABLE "admin_account_domains" ADD CONSTRAINT "admin_account_domains_admin_account_id_admin_accounts_id_fk" FOREIGN KEY ("admin_account_id") REFERENCES "public"."admin_accounts"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "admin_account_domains_domain_idx" ON "admin_account_domains" USING btree ("domain","admin_account_id");--> statement-breakpoint
CREATE UNIQUE INDEX "admin_accounts_username_lower_uidx" ON "admin_accounts" USING btree (lower("username"));