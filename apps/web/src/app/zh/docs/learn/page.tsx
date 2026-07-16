import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LearnHubView } from "@/components/learn-article";
import { buildHubJsonLd, buildHubMetadata } from "@/lib/learn-page";

export const metadata: Metadata = buildHubMetadata("zh");

export default function LearnHubPageZh() {
  return <><JsonLd data={buildHubJsonLd("zh")} /><LearnHubView locale="zh" /></>;
}
