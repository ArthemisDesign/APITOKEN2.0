import type { Metadata } from "next";
import VerifyEmailPage from "@/app/verify-email/page";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Подтверждение email", "Подтвердите адрес электронной почты для аккаунта apiToken.sale.");

export default VerifyEmailPage;
