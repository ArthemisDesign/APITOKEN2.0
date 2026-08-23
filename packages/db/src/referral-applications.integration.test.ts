import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  createDatabase,
  decideReferralApplication,
  findLatestReferralApplication,
  listReferralApplications,
  submitReferralApplication,
  type Database,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

/** The partner-access queue against real SQL: one open application, one decision, no rewrites. */
describe.runIf(Boolean(connectionString))("partner access applications", () => {
  let database: Database;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await database.pool.query("TRUNCATE users RESTART IDENTITY CASCADE");
    await database.pool.end();
  });

  beforeEach(async () => {
    await database.pool.query("TRUNCATE users RESTART IDENTITY CASCADE");
  });

  async function account(email: string): Promise<string> {
    const id = randomUUID();
    await database.pool.query(
      "INSERT INTO users(id, email, display_name, password_hash, status) VALUES($1, $2, $3, $4, 'active')",
      [id, email, email.split("@")[0], "x".repeat(60)],
    );
    return id;
  }

  it("keeps one open application per account and refreshes its message", async () => {
    const userId = await account(`applicant-${randomUUID().slice(0, 8)}@example.com`);

    const first = await submitReferralApplication(database, { userId, message: "Agency with three products." });
    const second = await submitReferralApplication(database, { userId, message: "Updated: also a newsletter." });

    expect(second.id).toBe(first.id);
    expect(second.message).toBe("Updated: also a newsletter.");
    expect((await listReferralApplications(database, { status: "pending" })).length).toBe(1);
  });

  it("reports the account email with the application and nothing internal", async () => {
    const email = `named-${randomUUID().slice(0, 8)}@example.com`;
    const userId = await account(email);
    await submitReferralApplication(database, { userId, message: "Community of 4k developers." });

    const mine = await findLatestReferralApplication(database, userId);

    expect(mine).toMatchObject({ email, status: "pending", reviewerActor: null, decidedAt: null });
  });

  it("decides once and refuses a second decision", async () => {
    const userId = await account(`decided-${randomUUID().slice(0, 8)}@example.com`);
    const application = await submitReferralApplication(database, { userId, message: "Integrator." });

    const approved = await decideReferralApplication(database, {
      id: application.id, status: "approved", reviewerActor: "ops", reviewerNote: "Known integrator.",
    });
    expect(approved).toMatchObject({ status: "approved", reviewerActor: "ops", reviewerNote: "Known integrator." });
    expect(approved?.decidedAt).not.toBeNull();

    const again = await decideReferralApplication(database, {
      id: application.id, status: "rejected", reviewerActor: "ops", reviewerNote: "Changed my mind.",
    });
    expect(again).toBeNull();
    expect((await findLatestReferralApplication(database, userId))?.status).toBe("approved");
  });

  it("lets a declined account apply again without touching the closed decision", async () => {
    const userId = await account(`retry-${randomUUID().slice(0, 8)}@example.com`);
    const first = await submitReferralApplication(database, { userId, message: "Too early." });
    await decideReferralApplication(database, {
      id: first.id, status: "rejected", reviewerActor: "ops", reviewerNote: "No traffic yet.",
    });

    const second = await submitReferralApplication(database, { userId, message: "Now with traffic." });

    expect(second.id).not.toBe(first.id);
    expect(second.status).toBe("pending");
    const all = await listReferralApplications(database, {});
    expect(all.map((item) => item.status)).toEqual(["pending", "rejected"]);
  });

  it("queues pending applications first for the admin review list", async () => {
    const older = await account(`older-${randomUUID().slice(0, 8)}@example.com`);
    const newer = await account(`newer-${randomUUID().slice(0, 8)}@example.com`);
    const decided = await submitReferralApplication(database, { userId: older, message: "First." });
    await decideReferralApplication(database, {
      id: decided.id, status: "approved", reviewerActor: "ops", reviewerNote: "Fine.",
    });
    await submitReferralApplication(database, { userId: newer, message: "Second." });

    const queue = await listReferralApplications(database, {});

    expect(queue[0]?.status).toBe("pending");
    expect(queue.map((item) => item.email)).toHaveLength(2);
  });
});
