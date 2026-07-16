import type { Metadata } from "next";
import Link from "next/link";
import { createNoIndexMetadata } from "@/lib/seo";

export const metadata: Metadata = createNoIndexMetadata("Page not found", "The requested apiToken.sale page does not exist.");

export default function NotFound() {
  return (
    <main className="auth-page">
      <div className="auth-card" style={{ textAlign: "center" }}>
        <span className="eyebrow">404</span>
        <h1>Page not found</h1>
        <p className="auth-sub">This page took a wrong turn. Your API key is unaffected.</p>
        <Link className="btn btn-primary" href="/">Back home</Link>
      </div>
    </main>
  );
}
