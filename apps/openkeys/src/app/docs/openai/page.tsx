import { redirect } from "next/navigation";
import { OFFICIAL_DOCS_OPENAI_URL } from "@/lib/connect-commands";

/** Ссылка из выдач ключей должна жить вечно; ведём на GPT-раздел официальных доков. */
export default function OpenAiDocsPage() {
  redirect(OFFICIAL_DOCS_OPENAI_URL);
}
