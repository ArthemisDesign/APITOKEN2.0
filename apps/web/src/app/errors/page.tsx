import type { Metadata } from "next";
import { ToolErrorsIndexPage, toolErrorsIndexMetadata } from "@/lib/tool-errors-page";

export const metadata: Metadata = toolErrorsIndexMetadata("en");

export default function Page() {
  return <ToolErrorsIndexPage locale="en" />;
}
