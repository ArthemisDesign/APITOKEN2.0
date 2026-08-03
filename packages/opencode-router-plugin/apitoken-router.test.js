import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import test from "node:test"

import {
  CACHE_FRESH_TTL_MS,
  CACHE_MAX_STALE_MS,
  discoverModels,
  readCapabilityCache,
  readConnection,
} from "./apitoken-router.js"

const KEY_A = "sk-pool-test-key-a"
const KEY_B = "sk-pool-test-key-b"
const BASE_A = "https://router.apitoken.sale/v1"
const BASE_B = "https://other-router.example/v1"
const NOW = Date.UTC(2026, 7, 3, 12, 0, 0)

function pricing() {
  const rate = { input: "1000000000", output: "2000000000", cache_read: "100000000", cache_write: "1200000000" }
  return { unit: "nano_usd_per_million_tokens", standard: rate, priority: rate }
}

function catalog() {
  return {
    data: [
      {
        id: "anthropic/claude-opus-5",
        name: "Claude Opus 5",
        owned_by: "anthropic",
        apitoken: {
          limits: { context: 1000000, input: 1000000, output: 128000 },
          capabilities: { reasoning_efforts: ["low", "medium", "high", "xhigh", "max"] },
          pricing: pricing(),
        },
      },
      {
        id: "openai/gpt-5.6",
        name: "GPT-5.6",
        owned_by: "openai",
        apitoken: {
          limits: { context: 400000, input: 272000, output: 128000 },
          capabilities: { reasoning_efforts: ["low", "medium", "high"], service_tiers: ["standard", "priority"] },
          pricing: pricing(),
        },
      },
    ],
  }
}

function successfulFetch() {
  return Promise.resolve({ ok: true, status: 200, json: async () => catalog() })
}

function failedFetch() {
  throw new Error("simulated outage")
}

function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "apitoken-opencode-cache-test-"))
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }))
  return path.join(directory, "catalog-v1.json")
}

test("same credential uses explicit stale capability-only models after a transient outage", async (t) => {
  const cachePath = fixture(t)
  const live = await discoverModels({ key: KEY_A, base: BASE_A, cachePath, fetchImpl: successfulFetch, now: NOW })
  assert.equal(live.source, "live")
  assert.equal(live.models["openai/gpt-5.6"].cost.input, 1)
  assert.equal(fs.statSync(cachePath).mode & 0o777, 0o600)

  const warnings = []
  const stale = await discoverModels({
    key: KEY_A,
    base: BASE_A,
    cachePath,
    fetchImpl: failedFetch,
    now: NOW + CACHE_FRESH_TTL_MS + 1,
    warn: (message) => warnings.push(message),
  })
  assert.equal(stale.source, "stale")
  assert.match(stale.models["anthropic/claude-opus-5"].name, /stale metadata; pricing unavailable/)
  assert.equal(stale.models["anthropic/claude-opus-5"].limit.output, 128000)
  assert.deepEqual(Object.keys(stale.models["anthropic/claude-opus-5"].variants), ["low", "medium", "high", "xhigh", "max"])
  assert.equal(stale.models["openai/gpt-5.6-fast"].options.service_tier, "priority")
  for (const model of Object.values(stale.models)) {
    assert.equal("cost" in model, false)
    assert.equal("pricing" in model, false)
  }
  assert.match(warnings.join("\n"), /using stale capability metadata.*without pricing/)

  const decrypted = readCapabilityCache({ cachePath, key: KEY_A, base: BASE_A, now: NOW + 1 })
  assert.equal(JSON.stringify(decrypted).includes("pricing"), false)
  assert.equal(JSON.stringify(decrypted).includes("cost"), false)
})

test("cache is rejected for a different credential or base URL", async (t) => {
  const cachePath = fixture(t)
  await discoverModels({ key: KEY_A, base: BASE_A, cachePath, fetchImpl: successfulFetch, now: NOW })

  for (const identity of [{ key: KEY_B, base: BASE_A }, { key: KEY_A, base: BASE_B }]) {
    const result = await discoverModels({
      ...identity,
      cachePath,
      fetchImpl: failedFetch,
      now: NOW + 1,
      warn: () => {},
    })
    assert.equal(result.source, "none")
    assert.deepEqual(result.models, {})
  }
})

test("tampered, expired, and version-mismatched cache fails closed", async (t) => {
  const cachePath = fixture(t)
  await discoverModels({ key: KEY_A, base: BASE_A, cachePath, fetchImpl: successfulFetch, now: NOW })
  const original = JSON.parse(fs.readFileSync(cachePath, "utf8"))

  const replacement = original.ciphertext[0] === "A" ? "B" : "A"
  const tampered = { ...original, ciphertext: `${replacement}${original.ciphertext.slice(1)}` }
  fs.writeFileSync(cachePath, JSON.stringify(tampered), { mode: 0o600 })
  assert.throws(
    () => readCapabilityCache({ cachePath, key: KEY_A, base: BASE_A, now: NOW + 1 }),
    /authentication failed/,
  )

  fs.writeFileSync(cachePath, JSON.stringify(original), { mode: 0o600 })
  assert.throws(
    () => readCapabilityCache({ cachePath, key: KEY_A, base: BASE_A, now: NOW + CACHE_MAX_STALE_MS + 1 }),
    /expired/,
  )

  fs.writeFileSync(cachePath, JSON.stringify({ ...original, schema: 2 }), { mode: 0o600 })
  assert.throws(
    () => readCapabilityCache({ cachePath, key: KEY_A, base: BASE_A, now: NOW + 1 }),
    /unsupported capability cache version/,
  )

  if (process.platform !== "win32") {
    fs.writeFileSync(cachePath, JSON.stringify(original), { mode: 0o600 })
    fs.chmodSync(cachePath, 0o640)
    assert.throws(
      () => readCapabilityCache({ cachePath, key: KEY_A, base: BASE_A, now: NOW + 1 }),
      /permissions are not 0600/,
    )
  }
})

test("connection discovery is scoped to the apitoken provider and resolves its env placeholder", (t) => {
  const cachePath = fixture(t)
  const configPath = path.join(path.dirname(cachePath), "opencode.jsonc")
  fs.writeFileSync(configPath, `{
    // A different provider must never lend its endpoint to the router credential.
    "provider": {
      "other": { "options": { "apiKey": "sk-pool-wrong", "baseURL": "https://wrong.example/v1" } },
      "apitoken": {
        "options": {
          "apiKey": "{env:ROUTER_TEST_KEY}",
          "baseURL": "https://router.apitoken.sale/v1/"
        }
      }
    }
  }`)
  assert.deepEqual(
    readConnection({ env: { ROUTER_TEST_KEY: KEY_A }, configPath }),
    { key: KEY_A, base: BASE_A },
  )
})

test("malformed live catalog does not replace a valid last-good cache", async (t) => {
  const cachePath = fixture(t)
  await discoverModels({ key: KEY_A, base: BASE_A, cachePath, fetchImpl: successfulFetch, now: NOW })
  const before = fs.readFileSync(cachePath, "utf8")
  const malformedFetch = async () => ({ ok: true, status: 200, json: async () => ({ data: "not-an-array" }) })

  const result = await discoverModels({
    key: KEY_A,
    base: BASE_A,
    cachePath,
    fetchImpl: malformedFetch,
    now: NOW + 1,
    warn: () => {},
  })
  assert.equal(result.source, "stale")
  assert.equal(fs.readFileSync(cachePath, "utf8"), before)
})
