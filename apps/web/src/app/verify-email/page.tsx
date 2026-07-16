import type { Metadata } from "next";
import { Suspense } from "react";
import { VerifyEmail } from "./verify-email";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Verify email", "Complete email verification for your apiToken.sale account.");
export default function VerifyEmailPage() { return <Suspense><VerifyEmail /></Suspense>; }
