"use client";

import { useEffect, useState, type ReactNode } from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { api, ApiError, type Partner } from "@/lib/api";
import { Brand, Loading } from "@/components/ui";
import { PartnerContext } from "@/components/partner-context";

const NAV = [
  { href: "/dashboard", label: "Overview", icon: "◧" },
  { href: "/dashboard/referrals", label: "Referrals", icon: "⇢" },
  { href: "/dashboard/team", label: "Team", icon: "⁂" },
  { href: "/dashboard/payouts", label: "Payouts", icon: "◈" },
  { href: "/dashboard/settings", label: "Settings", icon: "⚙" },
];

export default function DashboardLayout({ children }: { children: ReactNode }) {
  const router = useRouter();
  const pathname = usePathname();
  const [partner, setPartner] = useState<Partner | null>(null);
  const [checking, setChecking] = useState(true);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api<{ partner: Partner }>("/v1/auth/me");
        if (!cancelled) {
          setPartner(res.partner);
          setChecking(false);
        }
      } catch (err) {
        if (cancelled) return;
        if (err instanceof ApiError && err.status === 401) {
          router.replace("/login");
        } else {
          setChecking(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [router]);

  // Close the mobile menu on navigation.
  useEffect(() => {
    setMenuOpen(false);
  }, [pathname]);

  async function logout() {
    try {
      await api("/v1/auth/logout", { method: "POST" });
    } catch {
      // ignore — redirect regardless
    }
    router.replace("/login");
  }

  if (checking || !partner) {
    return (
      <div className="auth-shell" style={{ justifyContent: "center" }}>
        {checking ? (
          <Loading label="Loading your cabinet…" />
        ) : (
          <div className="auth-card">
            <h1>Can&apos;t reach the partner API</h1>
            <p className="auth-sub">Check your connection and reload the page.</p>
            <button className="btn btn-primary" onClick={() => window.location.reload()}>
              Reload
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <PartnerContext.Provider value={partner}>
      <div className="cab">
        <aside className={`cab-sidebar${menuOpen ? " open" : ""}`}>
          <Link href="/dashboard" className="brand">
            <Brand />
          </Link>
          <nav className="cab-nav">
            {NAV.map((item) => {
              const active =
                item.href === "/dashboard"
                  ? pathname === "/dashboard"
                  : pathname.startsWith(item.href);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  className={`cab-link${active ? " active" : ""}`}
                >
                  <span aria-hidden style={{ width: 18, textAlign: "center" }}>
                    {item.icon}
                  </span>
                  {item.label}
                </Link>
              );
            })}
          </nav>
          <div className="cab-side-foot">
            Partner code: <span className="mono">{partner.referralCode}</span>
          </div>
        </aside>
        {menuOpen ? (
          <div className="cab-overlay" onClick={() => setMenuOpen(false)} aria-hidden />
        ) : null}
        <div className="cab-main">
          <header className="cab-topbar">
            <button
              className="cab-burger"
              onClick={() => setMenuOpen((v) => !v)}
              aria-label="Toggle menu"
            >
              ☰
            </button>
            <div className="cab-topbar-user">
              <span className="email">
                {partner.telegramUsername ? `@${partner.telegramUsername}` : partner.displayName ?? partner.email}
              </span>
              <button className="btn btn-ghost btn-sm" onClick={logout}>
                Log out
              </button>
            </div>
          </header>
          <main className="cab-content">{children}</main>
        </div>
      </div>
    </PartnerContext.Provider>
  );
}
