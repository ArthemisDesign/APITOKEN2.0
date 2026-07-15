import type { Metadata } from "next";
import { PlansContent } from "@/components/marketing-pages";

export const metadata: Metadata = { title: "Pricing" };

export default function PricingPage() {
  return <PlansContent />;
}
