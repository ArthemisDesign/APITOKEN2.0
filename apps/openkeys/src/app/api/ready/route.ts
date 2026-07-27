import { NextResponse } from "next/server";
import { loadConfig } from "@/lib/config";
import { getDatabase } from "@/lib/db";
import { getEngineClient } from "@/lib/engine";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/** Readiness intentionally exposes no dependency details to the public internet. */
export async function GET(): Promise<NextResponse> {
  try {
    loadConfig();
    const { pool } = getDatabase();
    const [database, engine] = await Promise.all([
      pool.query("SELECT 1"),
      getEngineClient().readiness(),
    ]);
    if (database.rowCount !== 1 || !engine) throw new Error("dependency unavailable");
    return NextResponse.json({ status: "ready" }, {
      headers: { "cache-control": "no-store" },
    });
  } catch {
    return NextResponse.json({ status: "unavailable" }, {
      status: 503,
      headers: { "cache-control": "no-store" },
    });
  }
}
