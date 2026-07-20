-- Персональные ОДНОРАЗОВЫЕ ссылки со скидкой: обычная реф-ссылка партнёра ведёт по обычным b2c-тирам
-- (без спец-скидки), а спец-скидку партнёр (с правом) выпускает ПЕРСОНАЛЬНО под клиента — отдельным
-- кодом, который получает скидку и гасится первым же привязанным пользователем. Плюс промокоды могут
-- нести спец-скидку (discount_bps > 0). Скидка ≤ 90%.
CREATE TABLE "partner_discount_links" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "partner_id" uuid NOT NULL REFERENCES "partners"("id") ON DELETE restrict,
  "code" text NOT NULL,
  "discount_bps" integer NOT NULL,
  "consumed_by_commerce_user_id" uuid,
  "consumed_at" timestamptz,
  "created_at" timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT "partner_discount_links_discount_check" CHECK ("discount_bps" BETWEEN 0 AND 9000)
);--> statement-breakpoint
CREATE UNIQUE INDEX "partner_discount_links_code_uidx" ON "partner_discount_links" ("code");--> statement-breakpoint
CREATE INDEX "partner_discount_links_partner_idx" ON "partner_discount_links" ("partner_id", "created_at");--> statement-breakpoint
ALTER TABLE "promo_codes" ADD COLUMN "discount_bps" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "promo_codes" ADD CONSTRAINT "promo_codes_discount_check" CHECK ("discount_bps" BETWEEN 0 AND 9000);
