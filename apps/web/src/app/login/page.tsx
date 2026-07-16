import type { Metadata } from "next";
import { Suspense } from "react";
import { LoginForm } from "./login-form";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Log in", "Log in to your private apiToken.sale account.");
export default function LoginPage() { return <Suspense><LoginForm /></Suspense>; }
