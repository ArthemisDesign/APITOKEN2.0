"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { AuthIntro, Feedback } from "@/components/auth-shell";

export function OAuthCallback() {
  const search = useSearchParams();
  const router = useRouter();
  const error = search.get("error");
  const [message, setMessage] = useState(error ? `Sign-in failed: ${error.replaceAll("_", " ")}` : "Completing sign-in…");
  useEffect(() => {
    if (error) return;
    api.me().then(() => { router.replace("/dashboard"); })
      .catch(() => setMessage("The sign-in session could not be confirmed."));
  }, [error, router]);
  return <><AuthIntro title="Social sign-in" subtitle="Returning securely to apiToken.sale." />
    <Feedback message={message} success={!error} /><div className="auth-alt"><Link href="/login">Back to login</Link></div></>;
}
