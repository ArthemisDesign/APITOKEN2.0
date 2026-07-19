-- Реф-код (партнёрский), протянутый через OAuth-флоу: захватывается на begin, читается на
-- complete — чтобы реф партнёра стал B2B ДО выдачи welcome-бонуса (бонус ему не положен).
ALTER TABLE "oauth_transactions" ADD COLUMN "referral_code" text;
