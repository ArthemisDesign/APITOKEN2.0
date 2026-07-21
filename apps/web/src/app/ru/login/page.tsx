import type { Metadata } from "next";
import LoginPage from "@/app/login/page";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Вход", "Войдите в личный кабинет apiToken.sale.");

export default LoginPage;
