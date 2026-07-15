import type { Metadata } from "next";
import { DocsPortal } from "./docs-portal";

export const metadata: Metadata = {
  title: "Documentation",
  description: "Connect Claude tools and SDKs to apiToken.sale.",
};

export default function DocsPage() {
  return <DocsPortal />;
}
