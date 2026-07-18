import Link from "next/link";
import { Brand } from "@/components/ui";

export default function Landing() {
  return (
    <main className="gate">
      <div className="gate-card">
        <Brand />
        <p>
          Partner program for apitoken.sale — earn a share of what your
          referrals spend.
        </p>
        <div className="gate-actions">
          <Link href="/register" className="btn btn-primary btn-lg">
            Create account
          </Link>
          <Link href="/login" className="btn btn-ghost btn-lg">
            Sign in
          </Link>
        </div>
      </div>
      <p className="gate-foot">
        Main service:{" "}
        <a href="https://apitoken.sale" rel="noopener">
          apitoken.sale
        </a>
      </p>
    </main>
  );
}
