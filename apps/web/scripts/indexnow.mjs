// Submit site URLs to IndexNow (Yandex, Bing, Seznam, Naver share the endpoint).
// Usage:
//   node scripts/indexnow.mjs               # submit every URL from the live sitemap
//   node scripts/indexnow.mjs <url> [...]   # submit specific URLs only
// The key file must be live at https://apitoken.sale/<KEY>.txt before running.

const HOST = "apitoken.sale";
const KEY = "1308f989ba68a288dc5acf1423b445f8";
const ENDPOINT = "https://api.indexnow.org/indexnow";

async function sitemapUrls() {
  const response = await fetch(`https://${HOST}/sitemap.xml`);
  if (!response.ok) throw new Error(`sitemap fetch failed: ${response.status}`);
  const xml = await response.text();
  return [...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => match[1]);
}

const args = process.argv.slice(2);
const urlList = args.length > 0 ? args : await sitemapUrls();
console.log(`Submitting ${urlList.length} URLs to IndexNow…`);

// The protocol allows up to 10,000 URLs per POST; we are far below that.
const response = await fetch(ENDPOINT, {
  method: "POST",
  headers: { "content-type": "application/json; charset=utf-8" },
  body: JSON.stringify({
    host: HOST,
    key: KEY,
    keyLocation: `https://${HOST}/${KEY}.txt`,
    urlList,
  }),
});

console.log(`IndexNow response: ${response.status} ${response.statusText}`);
if (!response.ok && response.status !== 202) {
  console.error(await response.text());
  process.exit(1);
}
console.log("Done. 200/202 = accepted; engines crawl asynchronously.");
