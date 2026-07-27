import { redirect } from "next/navigation";

/** Ссылки первой волны выдавались как /u/<token>; они должны продолжать работать. */
export default async function LegacyKeyLinkPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;
  redirect(`/profile/${token}`);
}
