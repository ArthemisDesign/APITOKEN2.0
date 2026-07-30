import { redirect } from "next/navigation";
import { OFFICIAL_DOCS_URL } from "@/lib/connect-commands";

/**
 * Полные публичные доки живут на apitoken.sale и не требуют логина; OpenKeys
 * не поддерживает собственную копию, чтобы инструкции не расходились.
 */
export default function DocsPage() {
  redirect(OFFICIAL_DOCS_URL);
}
