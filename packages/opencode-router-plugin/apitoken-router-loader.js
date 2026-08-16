import { createHash, createPublicKey, verify } from "node:crypto"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"
import { pathToFileURL } from "node:url"

const CHANNEL_SCHEMA = 1
const CHANNEL_URL = "https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/channel-v1.json"
const RELEASE_ORIGIN = "https://raw.githubusercontent.com"
const RELEASE_PREFIX = "/apitokensale-admin/apitoken.sale/main/opencode/releases/"
const MANIFEST_MAX_BYTES = 16 * 1024
const RUNTIME_MAX_BYTES = 256 * 1024
const UPDATE_TIMEOUT_MS = 2000
const INSTALLER_FALLBACK_SHA256 = "cc485bf1d1c906402438a5c117d57562a833e603dd81b4c83aaae337e5377cec"
const STATE_KEYS = new Set(["schema", "highest", "active", "previous"])
const IDENTITY_KEYS = new Set(["sequence", "version", "sha256"])
const CHANNEL_KEYS = new Set(["schema", "sequence", "version", "sha256", "url", "signature"])
const SIGNING_PUBLIC_KEY = `-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEACEtWzLUnXWF/5NHkKrTTUcTNN7hLPmDX9npEv6Fw+uA=
-----END PUBLIC KEY-----`

function cacheRoot(env = process.env) {
  const root = env.XDG_CACHE_HOME || path.join(env.HOME || os.homedir(), ".cache")
  return path.join(root, "opencode", "apitoken-router", "runtime-v1")
}

function sha256(content) {
  return createHash("sha256").update(content).digest("hex")
}

function assertExactKeys(value, allowed, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`invalid ${label}`)
  for (const key of Object.keys(value)) if (!allowed.has(key)) throw new Error(`unexpected ${label}.${key}`)
  return value
}

function validateIdentity(value, label) {
  assertExactKeys(value, IDENTITY_KEYS, label)
  if (!Number.isSafeInteger(value.sequence) || value.sequence < 1) throw new Error(`invalid ${label}.sequence`)
  if (typeof value.version !== "string" || !/^[0-9]+\.[0-9]+\.[0-9]+$/.test(value.version)) throw new Error(`invalid ${label}.version`)
  if (typeof value.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(value.sha256)) throw new Error(`invalid ${label}.sha256`)
  return value
}

function channelPayload(channel) {
  return Buffer.from(JSON.stringify({
    schema: channel.schema,
    sequence: channel.sequence,
    version: channel.version,
    sha256: channel.sha256,
    url: channel.url,
  }))
}

function validateReleaseUrl(raw, version) {
  if (typeof raw !== "string") throw new Error("invalid channel.url")
  const url = new URL(raw)
  if (url.origin !== RELEASE_ORIGIN || url.username || url.password || url.port || url.search || url.hash) throw new Error("invalid channel.url")
  if (url.pathname !== `${RELEASE_PREFIX}apitoken-router-${version}.mjs`) throw new Error("channel version/url mismatch")
  return url.toString()
}

function validateChannel(value) {
  const channel = assertExactKeys(value, CHANNEL_KEYS, "channel")
  if (channel.schema !== CHANNEL_SCHEMA) throw new Error("unsupported channel schema")
  validateIdentity({ sequence: channel.sequence, version: channel.version, sha256: channel.sha256 }, "channel")
  channel.url = validateReleaseUrl(channel.url, channel.version)
  if (typeof channel.signature !== "string" || !/^[A-Za-z0-9_-]{86}$/.test(channel.signature)) throw new Error("invalid channel.signature")
  if (!verify(null, channelPayload(channel), createPublicKey(SIGNING_PUBLIC_KEY), Buffer.from(channel.signature, "base64url"))) {
    throw new Error("invalid channel signature")
  }
  return channel
}

function validateState(value) {
  const state = assertExactKeys(value, STATE_KEYS, "state")
  if (state.schema !== CHANNEL_SCHEMA) throw new Error("unsupported state schema")
  for (const field of ["highest", "active", "previous"]) {
    if (state[field] !== undefined && state[field] !== null) validateIdentity(state[field], `state.${field}`)
  }
  if (!state.highest) throw new Error("incomplete state")
  if (state.active && state.highest.sequence < state.active.sequence) throw new Error("invalid state sequence")
  return state
}

async function readLimited(response, maximum, label) {
  if (!response.ok) throw new Error(`${label} ${response.status}`)
  if (response.url) {
    const final = new URL(response.url)
    if (label === "channel" && final.toString() !== CHANNEL_URL) throw new Error("channel redirect rejected")
  }
  const declared = Number(response.headers?.get?.("content-length"))
  if (Number.isFinite(declared) && declared > maximum) throw new Error(`${label} exceeds size limit`)
  const content = Buffer.from(await response.arrayBuffer())
  if (content.length > maximum) throw new Error(`${label} exceeds size limit`)
  return content
}

async function fetchBytes(url, maximum, label, fetchImpl) {
  const response = await fetchImpl(url, {
    headers: { Accept: label === "channel" ? "application/json" : "text/javascript" },
    signal: AbortSignal.timeout(UPDATE_TIMEOUT_MS),
    cache: "no-store",
    redirect: "error",
  })
  return readLimited(response, maximum, label)
}

function runtimePath(root, digest) { return path.join(root, `${digest}.mjs`) }

function writeAtomic(target, content, mode = 0o600) {
  fs.mkdirSync(path.dirname(target), { recursive: true, mode: 0o700 })
  if (process.platform !== "win32") fs.chmodSync(path.dirname(target), 0o700)
  const temporary = path.join(path.dirname(target), `.${path.basename(target)}.${process.pid}.${Date.now()}.tmp`)
  let descriptor
  try {
    descriptor = fs.openSync(temporary, "wx", mode)
    fs.writeFileSync(descriptor, content)
    fs.fsyncSync(descriptor)
    fs.closeSync(descriptor)
    descriptor = undefined
    fs.renameSync(temporary, target)
    if (process.platform !== "win32") fs.chmodSync(target, mode)
  } catch (error) {
    if (descriptor !== undefined) fs.closeSync(descriptor)
    try { fs.unlinkSync(temporary) } catch {}
    throw error
  }
}

function readVerifiedRuntime(target, expected) {
  const stat = fs.lstatSync(target)
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("runtime is not a regular file")
  if (stat.size > RUNTIME_MAX_BYTES) throw new Error("runtime exceeds size limit")
  const content = fs.readFileSync(target)
  if (sha256(content) !== expected) throw new Error("runtime digest mismatch")
  return content
}

function readState(statePath) {
  try { return validateState(JSON.parse(fs.readFileSync(statePath, "utf8"))) } catch { return undefined }
}

async function executeRuntime(target, digest, input) {
  readVerifiedRuntime(target, digest)
  const imported = await import(`${pathToFileURL(target).href}?sha256=${digest}`)
  if (Object.keys(imported).join(",") !== "default" || typeof imported.default !== "function") throw new Error("runtime must expose only a default plugin factory")
  const hooks = await imported.default(input)
  if (hooks === null || typeof hooks !== "object" || Array.isArray(hooks)) throw new Error("runtime factory returned invalid hooks")
  return hooks
}

function identity(channel) {
  return { sequence: channel.sequence, version: channel.version, sha256: channel.sha256 }
}

function writeState(statePath, state) {
  const disk = readState(statePath)
  if (disk && disk.highest.sequence > state.highest.sequence) throw new Error("concurrent state advance detected")
  writeAtomic(statePath, Buffer.from(`${JSON.stringify(state)}\n`))
}

async function loadPlugin(input, {
  root = cacheRoot(),
  fallbackPath = path.join(root, "fallback.mjs"),
  fallbackSha256 = INSTALLER_FALLBACK_SHA256,
  manifestUrl = CHANNEL_URL,
  fetchImpl = fetch,
  verifyChannel = validateChannel,
  warn = (message) => console.warn(message),
} = {}) {
  const statePath = path.join(root, "state.json")
  let state = readState(statePath)
  let attempted
  try {
    const manifestBytes = await fetchBytes(manifestUrl, MANIFEST_MAX_BYTES, "channel", fetchImpl)
    const channel = verifyChannel(JSON.parse(manifestBytes.toString("utf8")))
    if (state && channel.sequence < state.highest.sequence) throw new Error("channel replay rejected")
    if (state && channel.sequence === state.highest.sequence && channel.sha256 !== state.highest.sha256) throw new Error("channel identity changed without a sequence increase")
    const seen = identity(channel)
    if (!state || channel.sequence > state.highest.sequence) {
      const watermark = state ? { ...state, highest: seen } : { schema: CHANNEL_SCHEMA, highest: seen }
      writeState(statePath, watermark)
      state = watermark
    }
    const target = runtimePath(root, channel.sha256)
    attempted = channel.sha256
    try { readVerifiedRuntime(target, channel.sha256) } catch {
      const runtime = await fetchBytes(channel.url, RUNTIME_MAX_BYTES, "runtime", fetchImpl)
      if (sha256(runtime) !== channel.sha256) throw new Error("downloaded runtime digest mismatch")
      writeAtomic(target, runtime)
    }
    const hooks = await executeRuntime(target, channel.sha256, input)
    const next = {
      schema: CHANNEL_SCHEMA,
      highest: seen,
      active: seen,
      previous: state?.active && state.active.sha256 !== seen.sha256 ? state.active : state?.previous,
    }
    writeState(statePath, next)
    return hooks
  } catch (error) {
    warn(`[apitoken-router] automatic update unavailable (${error.message}); trying last-good runtime`)
  }

  state = readState(statePath) ?? state
  for (const candidate of [state?.active, state?.previous]) {
    if (!candidate || candidate.sha256 === attempted) continue
    try { return await executeRuntime(runtimePath(root, candidate.sha256), candidate.sha256, input) }
    catch (error) { warn(`[apitoken-router] cached runtime rejected (${error.message})`) }
  }
  try {
    const content = fs.readFileSync(fallbackPath)
    if (content.length > RUNTIME_MAX_BYTES) throw new Error("fallback exceeds size limit")
    if (sha256(content) !== fallbackSha256) throw new Error("fallback digest mismatch")
    return await executeRuntime(fallbackPath, fallbackSha256, input)
  } catch (error) {
    warn(`[apitoken-router] no usable runtime (${error.message}); provider metadata plugin is disabled`)
    return {}
  }
}

export default async function ApitokenRouterLoader(input) { return loadPlugin(input) }

Object.defineProperty(ApitokenRouterLoader, "testing", {
  value: Object.freeze({ CHANNEL_URL, UPDATE_TIMEOUT_MS, cacheRoot, channelPayload, loadPlugin, sha256, validateChannel }),
})
