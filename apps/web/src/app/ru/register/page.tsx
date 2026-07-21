import type { Metadata } from "next";
import RegisterPage from "@/app/register/page";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Создать аккаунт", "Создайте аккаунт apiToken.sale и выпустите API-ключ.");

export default RegisterPage;
