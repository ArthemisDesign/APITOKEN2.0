"use client";

import { useEffect, useState } from "react";
import {
  api,
  ApiError,
  formatDate,
  formatUsd,
  type ReferralRow,
} from "@/lib/api";
import { EmptyState, Loading, Notice, Table } from "@/components/ui";

export default function ReferralsPage() {
  const [items, setItems] = useState<ReferralRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await api<{ items: ReferralRow[] }>("/v1/partner/referrals");
        if (!cancelled)
          setItems(
            [...res.items].sort((a, b) => (a.attributedAt < b.attributedAt ? 1 : -1)),
          );
      } catch (err) {
        if (!cancelled)
          setError(err instanceof ApiError ? err.message : "Failed to load referrals.");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <>
      <h1 className="page-title">Referrals</h1>
      <p className="page-sub">
        Users attributed to you. Identities are masked for their privacy — you see
        spend and your earnings per user.
      </p>
      {error ? <Notice kind="error">{error}</Notice> : null}
      {!items && !error ? <Loading label="Loading referrals…" /> : null}
      {items ? (
        items.length === 0 ? (
          <div className="card">
            <EmptyState title="No referrals yet">
              Share your referral link — every user who registers through it appears
              here.
            </EmptyState>
          </div>
        ) : (
          <Table
            head={
              <>
                <th>User</th>
                <th>Joined</th>
                <th className="num">Total spend</th>
                <th className="num">You earned</th>
              </>
            }
          >
            {items.map((r) => (
              <tr key={`${r.userMask}-${r.attributedAt}`}>
                <td className="mono">{r.userMask}</td>
                <td>{formatDate(r.attributedAt)}</td>
                <td className="num">{formatUsd(r.spendNano)}</td>
                <td className="num" style={{ color: "var(--accent-strong)", fontWeight: 700 }}>
                  {formatUsd(r.earnedNano)}
                </td>
              </tr>
            ))}
          </Table>
        )
      ) : null}
    </>
  );
}
