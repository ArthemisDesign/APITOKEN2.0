import type { Metadata } from "next";
import { JsonLd } from "@/components/json-ld";
import { LearnHubView } from "@/components/learn-article";
import { buildHubJsonLd, buildHubMetadata } from "@/lib/learn-page";

export const metadata: Metadata = buildHubMetadata("ko");

export default function LearnHubPageKo() {
  return <><JsonLd data={buildHubJsonLd("ko")} /><LearnHubView locale="ko" /></>;
}
