// Re-submit sitemap.xml to Google Search Console after a deploy.
// Google removed the anonymous sitemap ping (2023) and its Indexing API only
// accepts job postings/livestreams, so an authenticated GSC sitemap submit is
// the only legitimate automated path for regular pages.
//
// Requires env GOOGLE_SERVICE_ACCOUNT — the full JSON key of a service account
// added as an Owner of the GSC property. Optional GSC_SITE overrides the
// property id (default: URL-prefix property "https://apitoken.sale/").
//
// Usage: GOOGLE_SERVICE_ACCOUNT='{"client_email":...}' node scripts/google-sitemap.mjs

import { createSign } from "node:crypto";

const SITE = process.env.GSC_SITE ?? "https://apitoken.sale/";
const SITEMAP = "https://apitoken.sale/sitemap.xml";

const raw = process.env.GOOGLE_SERVICE_ACCOUNT;
if (!raw) {
  console.log("GOOGLE_SERVICE_ACCOUNT not set — skipping Google sitemap submit.");
  process.exit(0);
}
const sa = JSON.parse(raw);

const b64url = (input) =>
  Buffer.from(input).toString("base64url");

function makeJwt() {
  const now = Math.floor(Date.now() / 1000);
  const header = b64url(JSON.stringify({ alg: "RS256", typ: "JWT" }));
  const claims = b64url(
    JSON.stringify({
      iss: sa.client_email,
      scope: "https://www.googleapis.com/auth/webmasters",
      aud: "https://oauth2.googleapis.com/token",
      iat: now,
      exp: now + 3600,
    }),
  );
  const signer = createSign("RSA-SHA256");
  signer.update(`${header}.${claims}`);
  const signature = signer.sign(sa.private_key).toString("base64url");
  return `${header}.${claims}.${signature}`;
}

const tokenResponse = await fetch("https://oauth2.googleapis.com/token", {
  method: "POST",
  headers: { "content-type": "application/x-www-form-urlencoded" },
  body: new URLSearchParams({
    grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
    assertion: makeJwt(),
  }),
});
if (!tokenResponse.ok) {
  console.error(`token exchange failed: ${tokenResponse.status} ${await tokenResponse.text()}`);
  process.exit(1);
}
const { access_token } = await tokenResponse.json();

const submitUrl =
  `https://www.googleapis.com/webmasters/v3/sites/${encodeURIComponent(SITE)}` +
  `/sitemaps/${encodeURIComponent(SITEMAP)}`;
const submitResponse = await fetch(submitUrl, {
  method: "PUT",
  headers: { authorization: `Bearer ${access_token}` },
});
console.log(`GSC sitemap submit: ${submitResponse.status} ${submitResponse.statusText}`);
if (!submitResponse.ok) {
  console.error(await submitResponse.text());
  process.exit(1);
}
console.log(`Done. Google will re-read ${SITEMAP} for property ${SITE}.`);
