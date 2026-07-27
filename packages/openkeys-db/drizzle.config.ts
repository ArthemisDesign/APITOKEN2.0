import { defineConfig } from "drizzle-kit";

if (!process.env.OPENKEYS_DATABASE_URL) {
  throw new Error("OPENKEYS_DATABASE_URL is required");
}

export default defineConfig({
  dialect: "postgresql",
  schema: "./src/schema.ts",
  out: "./migrations",
  dbCredentials: { url: process.env.OPENKEYS_DATABASE_URL },
  strict: true,
  verbose: true,
});
