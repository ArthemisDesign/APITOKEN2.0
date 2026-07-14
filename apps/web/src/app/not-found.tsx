import Link from "next/link";

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
