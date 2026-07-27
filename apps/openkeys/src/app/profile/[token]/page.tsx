import { notFound } from "next/navigation";
import { KeyProfile } from "@/components/key-profile";
import { loadUsageByViewToken } from "@/lib/keys";

export const dynamic = "force-dynamic";
export const metadata = { title: "Профиль ключа — apiToken" };

/** Персональная ссылка, которую покупатель получает вместе с ключом. */
export default async function KeyProfilePage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;
  const view = await loadUsageByViewToken(token);
  if (!view) notFound();

  return <KeyProfile view={view} />;
}
