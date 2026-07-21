import type { Metadata } from "next";
import ForgotPasswordPage from "@/app/forgot-password/page";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Восстановление пароля", "Запросите ссылку для восстановления пароля apiToken.sale.");

export default ForgotPasswordPage;
