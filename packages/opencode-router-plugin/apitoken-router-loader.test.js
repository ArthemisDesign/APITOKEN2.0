import assert from "node:assert/strict"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import test from "node:test"

import Loader, * as loaderExports from "./apitoken-router-loader.js"

const { loadPlugin: rawLoadPlugin, sha256, validateChannel } = Loader.testing

function testChannel(value) {
  const { signature, ...channel } = value
  assert.equal(typeof signature, "string")
  return channel
}

function loadPlugin(input, options) {
  const fallback = path.join(options.root, "fallback.mjs")
  const fallbackSha256 = fs.existsSync(fallback) ? sha256(fs.readFileSync(fallback)) : undefined
  return rawLoadPlugin(input, { ...options, fallbackSha256, verifyChannel: testChannel })
}
const RELEASE_BASE = "https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/releases"

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "apitoken-loader-test-"))
  t.after(() => fs.rmSync(root, { recursive: true, force: true }))
  return root
}

function runtime(name, fail = false) {
  return Buffer.from(`export default async function () { ${fail ? 'throw new Error("broken factory")' : `return { marker: ${JSON.stringify(name)} }`} }\n`)
}

function channel(content, { sequence = 1, version = "1.0.0", digest = sha256(content) } = {}) {
  return {
    schema: 1,
    sequence,
    version,
    sha256: digest,
    url: `${RELEASE_BASE}/apitoken-router-${version}.mjs`,
    signature: "A".repeat(86),
  }
}

function response(content, status = 200) {
  return new Response(content, { status, headers: { "content-length": String(Buffer.byteLength(content)) } })
}

function fetchSequence(...steps) {
  const queue = [...steps]
  return async (url) => {
    const next = queue.shift()
    if (next instanceof Error) throw next
    assert.ok(next, `unexpected fetch ${url}`)
    if (typeof next === "function") return next(url)
    return response(next)
  }
}

function seedFallback(root, content = runtime("fallback")) {
  fs.mkdirSync(root, { recursive: true })
  fs.writeFileSync(path.join(root, "fallback.mjs"), content)
}

test("module exposes only the OpenCode plugin factory", () => {
  assert.deepEqual(Object.keys(loaderExports), ["default"])
  assert.equal(typeof loaderExports.default, "function")
})


test("accepts only a correctly signed manifest at the pinned release URL", () => {
  const signed = {"schema": 1, "sequence": 1, "version": "1.0.0", "sha256": "cc485bf1d1c906402438a5c117d57562a833e603dd81b4c83aaae337e5377cec", "url": "https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/releases/apitoken-router-1.0.0.mjs", "signature": "mSx0DrdXN9RYXvHkl2YUFl3mbVBjyeLsVjBoyo-iOmZDqLeiMkPi8x7lM_2jhBxpdAWNMOs4EAAKcoOPy0tiBg"}
  assert.equal(validateChannel({ ...signed }).sha256, signed.sha256)
  for (const mutation of [
    { ...signed, sha256: "0".repeat(64) },
    { ...signed, url: "https://raw.githubusercontent.com.evil.example/apitokensale-admin/apitoken.sale/main/opencode/releases/apitoken-router-1.0.0.mjs" },
    { ...signed, url: `${signed.url}?mutable=1` },
    { ...signed, sequence: 2 },
  ]) assert.throws(() => validateChannel(mutation))
})

test("downloads a verified release, adopts it, and uses it offline as last-good", async (t) => {
  const root = fixture(t)
  seedFallback(root)
  const content = runtime("release-1")
  const manifest = channel(content)
  const first = await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(manifest), content),
    warn: () => {},
  })
  assert.equal(first.marker, "release-1")
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, "state.json"), "utf8")), {
    schema: 1,
    highest: { sequence: 1, version: "1.0.0", sha256: manifest.sha256 },
    active: { sequence: 1, version: "1.0.0", sha256: manifest.sha256 },
  })
  assert.equal(fs.readFileSync(path.join(root, `${manifest.sha256}.mjs`), "utf8"), content.toString())

  const warnings = []
  const offline = await loadPlugin({}, {
    root,
    fetchImpl: async () => { throw new Error("offline") },
    warn: (message) => warnings.push(message),
  })
  assert.equal(offline.marker, "release-1")
  assert.match(warnings.join("\n"), /automatic update unavailable \(offline\)/)
})

test("digest mismatch never executes the download and falls back to installer seed", async (t) => {
  const root = fixture(t)
  seedFallback(root)
  const expected = runtime("expected")
  const malicious = runtime("must-not-run")
  const result = await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(channel(expected)), malicious),
    warn: () => {},
  })
  assert.equal(result.marker, "fallback")
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, "state.json"), "utf8")).highest.sequence, 1)
})

test("a failed candidate factory preserves and executes the previous release", async (t) => {
  const root = fixture(t)
  seedFallback(root)
  const good = runtime("release-1")
  const goodChannel = channel(good)
  assert.equal((await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(goodChannel), good),
    warn: () => {},
  })).marker, "release-1")

  const broken = runtime("release-2", true)
  const brokenChannel = channel(broken, { sequence: 2, version: "1.1.0" })
  const warnings = []
  const result = await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(brokenChannel), broken),
    warn: (message) => warnings.push(message),
  })
  assert.equal(result.marker, "release-1")
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, "state.json"), "utf8")).active.sequence, 1)
  assert.match(warnings.join("\n"), /broken factory/)
})

test("a failed higher sequence advances the watermark and rejects a later lower replay", async (t) => {
  const root = fixture(t)
  seedFallback(root)
  const good = runtime("release-1")
  await loadPlugin({}, { root, fetchImpl: fetchSequence(JSON.stringify(channel(good)), good), warn: () => {} })

  const broken = runtime("release-3", true)
  const high = channel(broken, { sequence: 3, version: "1.2.0" })
  assert.equal((await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(high), broken),
    warn: () => {},
  })).marker, "release-1")
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, "state.json"), "utf8")).highest.sequence, 3)

  const middle = channel(runtime("release-2"), { sequence: 2, version: "1.1.0" })
  assert.equal((await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(middle)),
    warn: () => {},
  })).marker, "release-1")
})

test("a lower sequence and a changed same-sequence identity are rejected as replay", async (t) => {
  const root = fixture(t)
  seedFallback(root)
  const good = runtime("release-2")
  const current = channel(good, { sequence: 2, version: "1.1.0" })
  await loadPlugin({}, { root, fetchImpl: fetchSequence(JSON.stringify(current), good), warn: () => {} })

  for (const replay of [
    channel(runtime("older"), { sequence: 1, version: "1.0.0" }),
    channel(runtime("replacement"), { sequence: 2, version: "1.1.0" }),
  ]) {
    const result = await loadPlugin({}, {
      root,
      fetchImpl: fetchSequence(JSON.stringify(replay)),
      warn: () => {},
    })
    assert.equal(result.marker, "release-2")
  }
})

test("unknown manifest fields and oversized payloads fail closed to fallback", async (t) => {
  const root = fixture(t)
  seedFallback(root)
  const content = runtime("release")
  const invalid = { ...channel(content), extra: true }
  assert.equal((await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(JSON.stringify(invalid)),
    warn: () => {},
  })).marker, "fallback")

  const oversized = "x".repeat(16 * 1024 + 1)
  assert.equal((await loadPlugin({}, {
    root,
    fetchImpl: fetchSequence(oversized),
    warn: () => {},
  })).marker, "fallback")
})
