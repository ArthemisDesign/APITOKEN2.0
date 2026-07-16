import type { Metadata } from "next";
import { Suspense } from "react";
import { ResetPasswordForm } from "./reset-password-form";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Reset password", "Set a new password for your apiToken.sale account.");
export default function ResetPasswordPage() { return <Suspense><ResetPasswordForm /></Suspense>; }
