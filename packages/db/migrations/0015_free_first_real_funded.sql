-- Free-first: чтобы реф-комиссия шла только с РЕАЛЬНЫХ денег, ведём на клиенте «бесплатный баланс»
-- (welcome-бонус, промо — по ref леджера) и по каждому списанию считаем real_funded = часть,
-- покрытую реальными деньгами (бесплатное тратится первым). free_balance=0 у всех при апгрейде —
-- на 13 текущих клиентов не влияет (они не рефералы; для новых рефов баланс копится с их кредитов).
ALTER TABLE "customer_profiles" ADD COLUMN "free_balance_nano" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_events" ADD COLUMN "real_funded_nano" bigint DEFAULT 0 NOT NULL;
