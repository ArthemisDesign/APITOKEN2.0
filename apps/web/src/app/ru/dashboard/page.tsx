import type { Metadata } from "next";
import DashboardPage from "../../dashboard/page";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata(
  "Панель управления",
  "Управление API-ключами, балансом, использованием и настройками аккаунта apiToken.sale.",
);

export default DashboardPage;
