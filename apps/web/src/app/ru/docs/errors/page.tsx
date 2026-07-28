import type { Metadata } from "next";
import { ErrorsReference } from "@/components/errors-reference";
import { errorsUi } from "@/lib/api-errors";
import { coreMetadata } from "@/lib/seo-core";

const EN = { title: errorsUi.en.title, description: errorsUi.en.description };
const RU = { title: errorsUi.ru.title, description: errorsUi.ru.description };

export const metadata: Metadata = {
  ...coreMetadata("/docs/errors", EN, RU, "ru"),
  keywords: [
    "коды ошибок claude api",
    "ошибка claude api",
    "invalid x-api-key",
    "claude api 429",
    "claude api 401",
    "лимит claude исчерпан",
    "claude api не работает",
  ],
};

export default function RuErrorsPage() {
  return <ErrorsReference locale="ru" />;
}
