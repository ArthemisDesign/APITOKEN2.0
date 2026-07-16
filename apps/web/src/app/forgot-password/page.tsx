import type { Metadata } from "next";
import { ForgotPasswordForm } from "./forgot-password-form";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Forgot password", "Request a password reset for your apiToken.sale account.");
export default function ForgotPasswordPage() { return <ForgotPasswordForm />; }
