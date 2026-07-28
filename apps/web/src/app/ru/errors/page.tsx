import type { Metadata } from "next";
import { ToolErrorsIndexPage, toolErrorsIndexMetadata } from "@/lib/tool-errors-page";

export const metadata: Metadata = toolErrorsIndexMetadata("ru");

export default function Page() {
  return <ToolErrorsIndexPage locale="ru" />;
}
