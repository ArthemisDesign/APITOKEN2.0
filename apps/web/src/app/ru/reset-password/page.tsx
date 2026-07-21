import type { Metadata } from "next";
import ResetPasswordPage from "@/app/reset-password/page";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Новый пароль", "Задайте новый пароль для аккаунта apiToken.sale.");

export default ResetPasswordPage;
