import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LearnHubView } from "@/components/learn-article";
import { buildHubJsonLd, buildHubMetadata } from "@/lib/learn-page";

export const metadata: Metadata = buildHubMetadata("ru");

export default function LearnHubPageRu() {
  return <><JsonLd data={buildHubJsonLd("ru")} /><LearnHubView locale="ru" /></>;
}
