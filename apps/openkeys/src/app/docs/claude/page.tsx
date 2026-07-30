import { redirect } from "next/navigation";
import { OFFICIAL_DOCS_URL } from "@/lib/connect-commands";

/** Ссылка из выдач ключей должна жить вечно; ведём на официальные полные доки. */
export default function ClaudeDocsPage() {
  redirect(OFFICIAL_DOCS_URL);
}
