import Link from "next/link";
import { Brand } from "@/components/ui";

export default function Landing() {
  return (
    <main className="gate">
      <div className="gate-card">
        <Brand />
        <p>
          Invite-only partner program for apitoken.sale — earn a share of what
          your referrals actually spend on API usage.
        </p>
        <div className="gate-actions">
          <Link href="/login" className="btn btn-primary btn-lg">
            Sign in with Telegram
          </Link>
          <Link href="/register" className="btn btn-ghost btn-lg">
            Apply to join
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
