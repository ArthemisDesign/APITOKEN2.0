import type { Metadata } from "next";
import { Suspense } from "react";
import { OAuthCallback } from "./oauth-callback";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Completing sign-in", "Complete secure social sign-in to apiToken.sale.");

export default function OAuthCallbackPage() { return <Suspense><OAuthCallback /></Suspense>; }
