import { describe, expect, it } from "vitest";
import { parseJournalLine } from "./journald.js";

function line(message: string, identifier = "apitoken-admin-deploy"): string {
  return JSON.stringify({ MESSAGE: message, SYSLOG_IDENTIFIER: identifier });
}

describe("parseJournalLine", () => {
  it("detects 'manual intervention required' as critical", () => {
    const event = parseJournalLine(line("[admin-deploy] rollback target also unhealthy — manual intervention required"));
    expect(event).toMatchObject({ source: "admin-deploy", severity: "critical" });
  });

  it("detects 'rolled back' and rollback.sh as warning", () => {
    expect(parseJournalLine(line("[watchdog] health check FAILED, rolled back to previous release")))
      .toMatchObject({ source: "watchdog", severity: "warning" });
    expect(parseJournalLine(line("[sales-deploy] invoking deploy/rollback.sh now")))
      .toMatchObject({ source: "sales-deploy", severity: "warning" });
  });

  it("detects manual retry as info", () => {
    expect(parseJournalLine(line("[agent-merge] retry requested by operator")))
      .toMatchObject({ source: "agent-merge", severity: "info" });
  });

  it("accepts known syslog identifiers without a bracket prefix", () => {
    const event = parseJournalLine(line("health check FAILED, rolled back", "watchdog"));
    expect(event).toMatchObject({ source: "watchdog", severity: "warning" });
  });

  it("ignores unknown sources, noise and malformed lines", () => {
    expect(parseJournalLine(line("[other-app] rolled back", "other"))).toBeNull();
    expect(parseJournalLine(line("[watchdog] polling origin/master…"))).toBeNull();
    expect(parseJournalLine("not json at all")).toBeNull();
    expect(parseJournalLine(JSON.stringify({ SYSLOG_IDENTIFIER: "watchdog" }))).toBeNull();
  });
});
