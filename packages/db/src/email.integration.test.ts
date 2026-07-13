import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  claimNextEmail,
  confirmEmail,
  createDatabase,
  createEmailUser,
  decodeAuthEncryptionKey,
  decryptAuthToken,
  encryptAuthToken,
  type Database,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;
const key = Buffer.alloc(32, 9);

describe.runIf(Boolean(connectionString))("durable authentication email", () => {
  let database: Database;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });
  beforeEach(clean);
  afterAll(async () => {
    await clean();
    await database.pool.end();
  });

  it("claims one encrypted message once and stores only the provider identity after delivery", async () => {
    const rawToken = "a".repeat(43);
    await createEmailUser(database, "mail@example.com", "password-hash", undefined, {
      tokenHash: "token-hash",
      encryptedToken: encryptAuthToken(rawToken, key),
      expiresAt: new Date(Date.now() + 86_400_000),
    });
    const [first, second] = await Promise.all([
      claimNextEmail(database, "worker-a"),
      claimNextEmail(database, "worker-b"),
    ]);
    const claimed = first ?? second;
    expect([first, second].filter(Boolean)).toHaveLength(1);
    expect(decryptAuthToken(claimed!.encryptedToken, decodeAuthEncryptionKey(key.toString("base64url")))).toBe(rawToken);
    await confirmEmail(database, claimed!.id, "smtp-message-id");
    const stored = await database.pool.query(`
      SELECT status, provider_message_id, locked_at, locked_by FROM email_outbox
    `);
    expect(stored.rows).toEqual([{
      status: "sent", provider_message_id: "smtp-message-id", locked_at: null, locked_by: null,
    }]);
  });

  async function clean(): Promise<void> {
    await database.pool.query(`
      TRUNCATE audit_log, email_outbox, oauth_transactions, auth_tokens, auth_sessions,
               auth_identities, pricing_months, customer_profiles, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
  }
});
