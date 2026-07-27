import { notFound } from "next/navigation";
import { loadUsageByViewToken } from "@/lib/keys";
import { KeyUsage } from "./key-usage";

export const dynamic = "force-dynamic";
export const metadata = { title: "Расход ключа — OpenKeys" };

export default async function KeyUsagePage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;
  const view = await loadUsageByViewToken(token);
  if (!view) notFound();

  return <KeyUsage view={view} />;
}
