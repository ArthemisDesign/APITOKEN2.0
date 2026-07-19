"use client";

import { useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatBps,
  formatDate,
  formatUsd,
  type EarningRow,
  type Overview,
  type ReferralRow,
} from "@/lib/api";
import {
  Card,
  CopyButton,
  EmptyState,
  Input,
  Loading,
  Notice,
} from "@/components/ui";
import { EarningsChart } from "@/components/earnings-chart";

export default function OverviewPage() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [earnings, setEarnings] = useState<EarningRow[] | null>(null);
  const [recent, setRecent] = useState<ReferralRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [ov, earn, refs] = await Promise.all([
          api<Overview>("/v1/partner/overview"),
          api<{ items: EarningRow[] }>("/v1/partner/earnings?days=30"),
          api<{ items: ReferralRow[] }>("/v1/partner/referrals"),
        ]);
        if (cancelled) return;
        setOverview(ov);
        setEarnings(earn.items);
        setRecent(
          [...refs.items]
            .sort((a, b) => (a.attributedAt < b.attributedAt ? 1 : -1))
            .slice(0, 6),
        );
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : "Failed to load overview.");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) return <Notice kind="error">{error}</Notice>;
  if (!overview || !earnings || !recent) return <Loading label="Loading overview…" />;

  return (
    <>
      <h1 className="page-title">Overview</h1>
      <p className="page-sub">
        Your terms: <strong>{formatBps(overview.commissionBps)}</strong> on what your
        referrals spend.
      </p>

      <div className="stat-grid">
        <div className="stat-card">
          <div className="stat-label">Available</div>
          <div className="stat-value green">{formatUsd(overview.totals.availableNano)}</div>
          <div className="stat-foot">
            {formatUsd(overview.totals.pendingPayoutNano)} pending payout
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Total earned</div>
          <div className="stat-value">{formatUsd(overview.totals.earnedNano)}</div>
          <div className="stat-foot">{formatUsd(overview.totals.paidNano)} paid out</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Referred users</div>
          <div className="stat-value">{overview.referredUsers}</div>
          <div className="stat-foot">
            {formatUsd(overview.last30d.spendNano)} spend in 30d
          </div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Earned in 30d</div>
          <div className="stat-value">{formatUsd(overview.last30d.earnedNano)}</div>
          <div className="stat-foot">rolling 30-day window</div>
        </div>
      </div>

      <div className="stack">
        <Card
          title="Your referral link"
          sub="Anyone who registers on apitoken.sale through this link is attributed to you permanently."
        >
          <div className="reflink-row">
            <Input readOnly value={overview.referralUrl} onFocus={(e) => e.currentTarget.select()} />
            <CopyButton value={overview.referralUrl} label="Copy link" />
          </div>
          <p className="field-hint" style={{ marginTop: 10 }}>
            Referral code: <span className="mono">{overview.referralCode}</span>
          </p>
        </Card>

        <Card
          title="Last 30 days"
          sub={`${formatUsd(overview.last30d.earnedNano)} earned on ${formatUsd(overview.last30d.spendNano)} of referral spend.`}
        >
          <EarningsChart items={earnings} />
        </Card>

        <Card title="Recent activity" sub="Latest referrals attributed to you.">
          {recent.length === 0 ? (
            <EmptyState title="No referrals yet">
              Share your link to start building your list.
            </EmptyState>
          ) : (
            <div className="activity-list">
              {recent.map((r) => (
                <div className="activity-item" key={`${r.userMask}-${r.attributedAt}`}>
                  <span>
                    <span className="mono">{r.userMask}</span> joined ·{" "}
                    <span style={{ color: "var(--accent-strong)" }}>
                      {formatUsd(r.earnedNano)}
                    </span>{" "}
                    earned for you
                  </span>
                  <span className="activity-date">{formatDate(r.attributedAt)}</span>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>
    </>
  );
}
