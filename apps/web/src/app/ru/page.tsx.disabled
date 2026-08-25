import type { Metadata } from "next";
import HomePage from "../page";
import { seoPages } from "@/lib/seo";
import { coreMetadata, coreRu } from "@/lib/seo-core";

export const metadata: Metadata = coreMetadata(
  "/",
  { title: seoPages.home.title, description: seoPages.home.description },
  coreRu.home,
  "ru",
);

export default function RuHomePage() {
  return <HomePage />;
}
