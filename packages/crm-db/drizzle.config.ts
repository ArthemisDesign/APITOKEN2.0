import { defineConfig } from "drizzle-kit";

if (!process.env.CRM_DATABASE_URL) {
  throw new Error("CRM_DATABASE_URL is required");
}

export default defineConfig({
  dialect: "postgresql",
  schema: "./src/schema.ts",
  out: "./migrations",
  dbCredentials: { url: process.env.CRM_DATABASE_URL },
  strict: true,
  verbose: true,
});
