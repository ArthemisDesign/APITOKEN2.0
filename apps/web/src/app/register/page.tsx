import type { Metadata } from "next";
import { Suspense } from "react";
import { RegisterForm } from "./register-form";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Create an account", "Create a private apiToken.sale account and generate your API key.");
export default function RegisterPage() { return <Suspense><RegisterForm /></Suspense>; }
