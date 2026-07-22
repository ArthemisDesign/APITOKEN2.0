ALTER TABLE "admin_account_domains" DROP CONSTRAINT "admin_account_domains_domain_check";--> statement-breakpoint
ALTER TABLE "admin_account_domains" ADD CONSTRAINT "admin_account_domains_domain_check" CHECK ("admin_account_domains"."domain" IN (
  'admin.apitoken.sale',
  'admin.partners.apitoken.sale',
  'crm.apitoken.sale',
  'content-studio.apitoken.sale',
  'monitoring.apitoken.sale'
));--> statement-breakpoint
INSERT INTO "admin_account_domains" ("admin_account_id", "domain")
SELECT "admin_account_id", 'monitoring.apitoken.sale'
FROM "admin_account_domains"
WHERE "domain" = 'admin.apitoken.sale'
ON CONFLICT ("admin_account_id", "domain") DO NOTHING;
