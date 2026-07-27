import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  claimSignupBonus,
  countRecentSubnetSignups,
  createDatabase,
  flagSignupProfile,
  recordDeviceSighting,
  releaseSignupBonus,
  upsertSignupProfile,
  type Database,
  type SignupProfileInput,
} from "./index.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("signup bonus antifraud persistence", () => {
  let database: Database;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query("TRUNCATE users RESTART IDENTITY CASCADE");
  });

  afterAll(async () => {
    await database.pool.query("TRUNCATE users RESTART IDENTITY CASCADE");
    await database.pool.end();
  });

  async function createUser(label: string): Promise<string> {
    const userId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)",
      [userId, `${label}-${userId}@gmail.com`, `Antifraud ${label}`],
    );
    return userId;
  }

  function profile(
    userId: string,
    label: string,
    overrides: Partial<SignupProfileInput> = {},
  ): SignupProfileInput {
    return {
      userId,
      emailCanonical: `${label}-${userId}@gmail.com`,
      ipAddress: `203.0.${label.length}.${label.length + 10}`,
      ipSubnet: `203.0.${label.length}.0/24`,
      userAgent: `antifraud-test/${label}`,
      deviceHash: `device-${label}-${userId}`,
      ...overrides,
    };
  }

  it.each([
    ["device", "duplicate-device"],
    ["subnet", "duplicate-subnet"],
    ["email", "duplicate-email"],
  ] as const)(
    "allows only one bonus for a shared %s and persists the denial reason",
    async (sharedSignal, expectedReason) => {
      const firstUserId = await createUser(`first-${sharedSignal}`);
      const secondUserId = await createUser(`second-${sharedSignal}`);
      const first = profile(firstUserId, `first-${sharedSignal}`);
      const second = profile(secondUserId, `second-${sharedSignal}`);

      if (sharedSignal === "device") second.deviceHash = first.deviceHash;
      if (sharedSignal === "subnet") second.ipSubnet = first.ipSubnet;
      if (sharedSignal === "email") second.emailCanonical = first.emailCanonical;

      await upsertSignupProfile(database, first);
      await upsertSignupProfile(database, second);

      await expect(claimSignupBonus(database, firstUserId)).resolves.toEqual({ claimed: true });
      await expect(claimSignupBonus(database, secondUserId)).resolves.toEqual({
        claimed: false,
        reason: expectedReason,
      });

      const stored = await database.pool.query<{
        bonus_granted: boolean;
        flagged_reason: string | null;
      }>(
        "SELECT bonus_granted, flagged_reason FROM signup_profiles WHERE user_id = $1",
        [secondUserId],
      );
      expect(stored.rows).toEqual([{
        bonus_granted: false,
        flagged_reason: expectedReason,
      }]);
    },
  );

  it("serializes simultaneous claims in one device cluster to exactly one winner", async () => {
    const firstUserId = await createUser("race-first");
    const secondUserId = await createUser("race-second");
    const sharedDevice = `race-device-${randomUUID()}`;
    await Promise.all([
      upsertSignupProfile(database, profile(firstUserId, "race-first", { deviceHash: sharedDevice })),
      upsertSignupProfile(database, profile(secondUserId, "race-second", { deviceHash: sharedDevice })),
    ]);

    const outcomes = await Promise.all([
      claimSignupBonus(database, firstUserId),
      claimSignupBonus(database, secondUserId),
    ]);
    expect(outcomes.filter((outcome) => outcome.claimed)).toHaveLength(1);
    expect(outcomes.filter((outcome) => !outcome.claimed)).toEqual([{
      claimed: false,
      reason: "duplicate-device",
    }]);

    const persisted = await database.pool.query<{
      bonus_granted: boolean;
      flagged_reason: string | null;
    }>(`
      SELECT bonus_granted, flagged_reason
      FROM signup_profiles
      WHERE user_id = ANY($1::uuid[])
      ORDER BY bonus_granted DESC
    `, [[firstUserId, secondUserId]]);
    expect(persisted.rows).toEqual([
      { bonus_granted: true, flagged_reason: null },
      { bonus_granted: false, flagged_reason: "duplicate-device" },
    ]);
  });

  it("never grants a profile after it has been flagged", async () => {
    const userId = await createUser("flagged");
    await upsertSignupProfile(database, profile(userId, "flagged"));
    await flagSignupProfile(database, userId, "subnet-velocity");

    await expect(claimSignupBonus(database, userId)).resolves.toEqual({
      claimed: false,
      reason: "already-granted",
    });
    const stored = await database.pool.query(
      "SELECT bonus_granted, flagged_reason FROM signup_profiles WHERE user_id = $1",
      [userId],
    );
    expect(stored.rows).toEqual([{
      bonus_granted: false,
      flagged_reason: "subnet-velocity",
    }]);
  });

  it("re-opens a claim only after explicit release compensation", async () => {
    const userId = await createUser("release");
    await upsertSignupProfile(database, profile(userId, "release"));

    await expect(claimSignupBonus(database, userId)).resolves.toEqual({ claimed: true });
    await expect(claimSignupBonus(database, userId)).resolves.toEqual({
      claimed: false,
      reason: "already-granted",
    });
    await releaseSignupBonus(database, userId);
    await expect(claimSignupBonus(database, userId)).resolves.toEqual({ claimed: true });
  });

  it("fills missing signup signals once without replacing the first observed identity", async () => {
    const userId = await createUser("profile");
    const first = profile(userId, "profile", {
      emailCanonical: "first@gmail.com",
      ipAddress: null,
      ipSubnet: null,
      userAgent: "first-agent",
      deviceHash: "first-device",
    });
    const second = profile(userId, "profile-second", {
      emailCanonical: "replacement@gmail.com",
      ipAddress: "198.51.100.44",
      ipSubnet: "198.51.100.0/24",
      userAgent: "replacement-agent",
      deviceHash: "replacement-device",
    });

    await expect(upsertSignupProfile(database, first)).resolves.toEqual({
      bonusGranted: false,
      flaggedReason: null,
    });
    await expect(upsertSignupProfile(database, second)).resolves.toEqual({
      bonusGranted: false,
      flaggedReason: null,
    });

    const stored = await database.pool.query(`
      SELECT email_canonical, ip_address, ip_subnet, user_agent, device_hash
      FROM signup_profiles
      WHERE user_id = $1
    `, [userId]);
    expect(stored.rows).toEqual([{
      email_canonical: "first@gmail.com",
      ip_address: "198.51.100.44",
      ip_subnet: "198.51.100.0/24",
      user_agent: "first-agent",
      device_hash: "first-device",
    }]);
  });

  it("counts only recent signup profiles in the requested subnet", async () => {
    const subnet = "192.0.2.0/24";
    const userIds = await Promise.all([
      createUser("recent-one"),
      createUser("recent-two"),
      createUser("expired"),
    ]);
    await Promise.all(userIds.map((userId, index) =>
      upsertSignupProfile(database, profile(userId, `subnet-${index}`, { ipSubnet: subnet })),
    ));
    await database.pool.query(
      "UPDATE signup_profiles SET created_at = now() - interval '2 hours' WHERE user_id = $1",
      [userIds[2]],
    );

    await expect(countRecentSubnetSignups(database, subnet, 60)).resolves.toBe(2);
    await expect(countRecentSubnetSignups(database, "198.18.0.0/24", 60)).resolves.toBe(0);
  });

  it("upserts each device-to-user sighting without duplicating the association", async () => {
    const firstUserId = await createUser("sighting-first");
    const secondUserId = await createUser("sighting-second");
    const deviceHash = `sighting-${randomUUID()}`;
    await recordDeviceSighting(database, deviceHash, firstUserId);
    await database.pool.query(`
      UPDATE device_sightings
      SET first_seen_at = '2020-01-01T00:00:00Z', last_seen_at = '2020-01-01T00:00:00Z'
      WHERE device_hash = $1 AND user_id = $2
    `, [deviceHash, firstUserId]);

    await recordDeviceSighting(database, deviceHash, firstUserId);
    await recordDeviceSighting(database, deviceHash, secondUserId);

    const stored = await database.pool.query<{
      user_id: string;
      first_seen_at: Date;
      last_seen_at: Date;
    }>(`
      SELECT user_id, first_seen_at, last_seen_at
      FROM device_sightings
      WHERE device_hash = $1
      ORDER BY user_id
    `, [deviceHash]);
    expect(stored.rows).toHaveLength(2);
    const repeated = stored.rows.find((row) => row.user_id === firstUserId)!;
    expect(repeated.first_seen_at.toISOString()).toBe("2020-01-01T00:00:00.000Z");
    expect(repeated.last_seen_at.getTime()).toBeGreaterThan(repeated.first_seen_at.getTime());
  });
});
