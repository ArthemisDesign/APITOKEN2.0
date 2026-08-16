import {
  createCipheriv,
  createDecipheriv,
  createHash,
  createHmac,
  hkdfSync,
  randomBytes,
  timingSafeEqual,
} from "node:crypto"
import fs from "node:fs"
import os from "node:os"
import path from "node:path"

const DEFAULT_BASE = "https://router.apitoken.sale/v1"
const CACHE_SCHEMA = 2
const CACHE_FRESH_TTL_MS = 15 * 60 * 1000
const CACHE_MAX_STALE_MS = 7 * 24 * 60 * 60 * 1000

const CACHE_DOMAIN = "apitoken-opencode-capability-cache-v2"
const CACHE_ALGORITHM = "aes-256-gcm"
const CACHE_CLOCK_SKEW_MS = 5 * 60 * 1000
const PRICING_UNIT = "nano_usd_per_million_tokens"
const NANO_USD_PER_USD = 1_000_000_000
const REASONING_EFFORTS = new Set(["none", "minimal", "low", "medium", "high", "xhigh", "max"])
const SERVICE_TIERS = new Set(["standard", "priority"])
const MODALITIES = new Set(["text", "image", "audio", "video", "pdf"])
const CACHE_RECORD_KEYS = new Set([
  "id",
  "name",
  "owned_by",
  "limits",
  "reasoning_efforts",
  "service_tiers",
  "input_modalities",
  "output_modalities",
  "tool_calling",
  "structured_outputs",
  "reasoning",
  "streaming",
])
const CACHE_ENVELOPE_KEYS = new Set([
  "schema",
  "algorithm",
  "identity",
  "fetched_at",
  "fresh_until",
  "stale_until",
  "iv",
  "tag",
  "ciphertext",
])

function configRoot(env = process.env) {
  return env.XDG_CONFIG_HOME || path.join(env.HOME || os.homedir(), ".config")
}

function defaultConfigPath(env = process.env) {
  return env.OPENCODE_CONFIG || path.join(configRoot(env), "opencode", "opencode.jsonc")
}

function defaultCachePath(env = process.env) {
  const root = env.XDG_CACHE_HOME || path.join(env.HOME || os.homedir(), ".cache")
  return path.join(root, "opencode", "apitoken-router", "catalog-v2.json")
}

function normalizeBase(base) {
  const url = new URL(base)
  if (url.protocol !== "https:" && url.protocol !== "http:") throw new Error("baseURL must use HTTP(S)")
  url.hash = ""
  url.search = ""
  url.pathname = url.pathname.replace(/\/+$/, "")
  return url.toString().replace(/\/$/, "")
}

function decodeJsonString(value) {
  try {
    return JSON.parse(`"${value}"`)
  } catch {
    return undefined
  }
}

function extractObjectProperty(text, property) {
  const match = new RegExp(`"${property}"\\s*:\\s*\\{`, "g").exec(text)
  if (!match) return undefined
  const start = text.indexOf("{", match.index)
  let depth = 0
  let inString = false
  let escaped = false
  let lineComment = false
  let blockComment = false
  for (let index = start; index < text.length; index += 1) {
    const current = text[index]
    const next = text[index + 1]
    if (lineComment) {
      if (current === "\n") lineComment = false
      continue
    }
    if (blockComment) {
      if (current === "*" && next === "/") {
        blockComment = false
        index += 1
      }
      continue
    }
    if (inString) {
      if (escaped) escaped = false
      else if (current === "\\") escaped = true
      else if (current === "\"") inString = false
      continue
    }
    if (current === "/" && next === "/") {
      lineComment = true
      index += 1
      continue
    }
    if (current === "/" && next === "*") {
      blockComment = true
      index += 1
      continue
    }
    if (current === "\"") inString = true
    else if (current === "{") depth += 1
    else if (current === "}") {
      depth -= 1
      if (depth === 0) return text.slice(start, index + 1)
    }
  }
  return undefined
}

function readConnection({ env = process.env, configPath = defaultConfigPath(env) } = {}) {
  try {
    const text = fs.readFileSync(configPath, "utf8")
    const provider = extractObjectProperty(text, "apitoken")
    if (!provider) throw new Error("apitoken provider is not configured")
    const rawKey = decodeJsonString(provider.match(/"apiKey"\s*:\s*"((?:\\.|[^"\\])*)"/)?.[1] ?? "")
    const envName = rawKey?.match(/^\{env:([A-Za-z_][A-Za-z0-9_]*)\}$/)?.[1]
    const key = envName ? env[envName] : rawKey
    const rawBase = decodeJsonString(provider.match(/"baseURL"\s*:\s*"((?:\\.|[^"\\])*)"/)?.[1] ?? "")
    const base = normalizeBase(rawBase || DEFAULT_BASE)
    return { key: typeof key === "string" && key.startsWith("sk-pool-") ? key : undefined, base }
  } catch {
    return { key: undefined, base: DEFAULT_BASE }
  }
}

function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`invalid ${label}`)
  }
  return value
}

function assertExactKeys(value, allowed, label) {
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) throw new Error(`unexpected ${label}.${key}`)
  }
}

function modelLimits(entry) {
  const source = entry.apitoken?.limits
  if (source === undefined) return undefined
  assertPlainObject(source, `${entry.id} limits`)
  const limit = {}
  for (const field of ["context", "input", "output"]) {
    const value = source[field]
    if (value === undefined) continue
    if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`invalid ${entry.id} limit.${field}`)
    limit[field] = value
  }
  return Object.keys(limit).length > 0 ? limit : undefined
}

function modelCapability(entry, field, allowed) {
  const value = entry.apitoken?.capabilities?.[field]
  if (value === undefined) return undefined
  if (!Array.isArray(value) || new Set(value).size !== value.length || value.some((item) => !allowed.has(item))) {
    throw new Error(`invalid ${entry.id} ${field}`)
  }
  return [...value]
}

function modelBooleanCapability(entry, field) {
  const value = entry.apitoken?.capabilities?.[field]
  if (value === undefined) return undefined
  if (typeof value !== "boolean") throw new Error(`invalid ${entry.id} ${field}`)
  return value
}

function capabilityRecord(entry) {
  assertPlainObject(entry, "catalog model")
  if (typeof entry.id !== "string" || entry.id.length === 0) throw new Error("catalog model has no id")
  if (typeof entry.owned_by !== "string" || entry.owned_by.length === 0) {
    throw new Error(`invalid ${entry.id} owned_by`)
  }
  if (entry.name !== undefined && (typeof entry.name !== "string" || entry.name.length === 0)) {
    throw new Error(`invalid ${entry.id} name`)
  }
  const reasoningEfforts = modelCapability(entry, "reasoning_efforts", REASONING_EFFORTS)
  const explicitReasoning = modelBooleanCapability(entry, "reasoning")
  if (explicitReasoning === false && reasoningEfforts?.some((effort) => effort !== "none")) {
    throw new Error(`contradictory ${entry.id} reasoning capabilities`)
  }
  return {
    id: entry.id,
    name: entry.name,
    owned_by: entry.owned_by,
    limits: modelLimits(entry),
    reasoning_efforts: reasoningEfforts,
    service_tiers: modelCapability(entry, "service_tiers", SERVICE_TIERS),
    input_modalities: modelCapability(entry, "input_modalities", MODALITIES),
    output_modalities: modelCapability(entry, "output_modalities", MODALITIES),
    tool_calling: modelBooleanCapability(entry, "tool_calling"),
    structured_outputs: modelBooleanCapability(entry, "structured_outputs"),
    reasoning: explicitReasoning ?? reasoningEfforts?.some((effort) => effort !== "none"),
    streaming: modelBooleanCapability(entry, "streaming"),
  }
}

function validateRecord(value) {
  const record = assertPlainObject(value, "cached model")
  assertExactKeys(record, CACHE_RECORD_KEYS, "cached model")
  if (typeof record.id !== "string" || record.id.length === 0) throw new Error("cached model has no id")
  if (record.name !== undefined && (typeof record.name !== "string" || record.name.length === 0)) {
    throw new Error(`invalid cached ${record.id} name`)
  }
  if (typeof record.owned_by !== "string" || record.owned_by.length === 0) {
    throw new Error(`invalid cached ${record.id} owned_by`)
  }
  const syntheticEntry = {
    id: record.id,
    apitoken: {
      limits: record.limits,
      capabilities: {
        reasoning_efforts: record.reasoning_efforts,
        service_tiers: record.service_tiers,
        input_modalities: record.input_modalities,
        output_modalities: record.output_modalities,
        tool_calling: record.tool_calling,
        structured_outputs: record.structured_outputs,
        reasoning: record.reasoning,
        streaming: record.streaming,
      },
    },
  }
  return {
    id: record.id,
    name: record.name,
    owned_by: record.owned_by,
    limits: modelLimits(syntheticEntry),
    reasoning_efforts: modelCapability(syntheticEntry, "reasoning_efforts", REASONING_EFFORTS),
    service_tiers: modelCapability(syntheticEntry, "service_tiers", SERVICE_TIERS),
    input_modalities: modelCapability(syntheticEntry, "input_modalities", MODALITIES),
    output_modalities: modelCapability(syntheticEntry, "output_modalities", MODALITIES),
    tool_calling: modelBooleanCapability(syntheticEntry, "tool_calling"),
    structured_outputs: modelBooleanCapability(syntheticEntry, "structured_outputs"),
    reasoning: modelBooleanCapability(syntheticEntry, "reasoning"),
    streaming: modelBooleanCapability(syntheticEntry, "streaming"),
  }
}

function effortVariants(efforts) {
  const variants = {}
  for (const effort of efforts) variants[effort] = { reasoningEffort: effort }
  return variants
}

function openCodeLimits(limits) {
  if (limits?.context === undefined || limits.output === undefined) return undefined
  return limits
}

function describe(record, stale = false) {
  const {
    id,
    name,
    owned_by: ownedBy,
    limits,
    reasoning_efforts: efforts,
    input_modalities: inputModalities,
    output_modalities: outputModalities,
    tool_calling: toolCalling,
    structured_outputs: structuredOutputs,
    reasoning,
  } = record
  if (ownedBy === "router") return null
  const bare = id.includes("/") ? id.slice(id.indexOf("/") + 1) : id
  const displayName = name ?? bare
  // The OpenAI-compatible transport used by OpenCode cannot decode generated-image payloads.
  // Keep every other authoritative modality, but remove image output from this consumer view.
  const compatibleOutput = outputModalities?.filter((modality) => modality !== "image")
  if (outputModalities !== undefined && compatibleOutput.length === 0) return null
  const base = {
    name: stale ? `${displayName} [stale metadata; pricing unavailable]` : displayName,
    ...(openCodeLimits(limits) ? { limit: limits } : {}),
    ...(toolCalling !== undefined ? { tool_call: toolCalling } : {}),
    ...(structuredOutputs !== undefined ? { structured_output: structuredOutputs } : {}),
    ...(inputModalities !== undefined ? { attachment: inputModalities.includes("image") } : {}),
    ...(reasoning !== undefined ? { reasoning } : {}),
    ...(reasoning ? { interleaved: { field: "reasoning_content" } } : {}),
    ...(efforts?.length > 0 ? { variants: effortVariants(efforts) } : {}),
    ...(inputModalities !== undefined && compatibleOutput !== undefined
      ? { modalities: { input: inputModalities, output: compatibleOutput } }
      : {}),
  }
  return base
}

function usdPerMillion(value, field) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)) {
    throw new Error(`invalid ${field} pricing`)
  }
  const nano = BigInt(value)
  if (nano > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error(`${field} pricing exceeds OpenCode numeric range`)
  }
  return Number(nano) / NANO_USD_PER_USD
}

function modelCost(pricing, tier) {
  if (pricing?.unit !== PRICING_UNIT || !pricing?.[tier]) throw new Error(`missing ${tier} pricing`)
  const convert = (rate, label) => ({
    input: usdPerMillion(rate.input, `${label}.input`),
    output: usdPerMillion(rate.output, `${label}.output`),
    cache_read: usdPerMillion(rate.cache_read, `${label}.cache_read`),
    cache_write: usdPerMillion(rate.cache_write, `${label}.cache_write`),
  })
  const cost = convert(pricing[tier], tier)
  if (pricing.long_context?.threshold_tokens === 200000 && pricing.long_context?.[tier]) {
    cost.context_over_200k = convert(pricing.long_context[tier], `long_context.${tier}`)
  }
  return cost
}

function liveModels(entries, records) {
  const models = {}
  for (let index = 0; index < entries.length; index += 1) {
    const entry = entries[index]
    const record = records[index]
    const model = describe(record)
    if (!model) continue
    const pricing = entry.apitoken?.pricing
    models[entry.id] = { ...model, cost: modelCost(pricing, "standard") }
    if (record.service_tiers?.includes("priority")) {
      models[`${entry.id}-fast`] = {
        ...model,
        id: entry.id,
        name: `${model.name} Fast`,
        cost: modelCost(pricing, "priority"),
        options: { service_tier: "priority" },
      }
    }
  }
  return models
}

function cachedModels(records) {
  const models = {}
  for (const record of records) {
    const model = describe(record, true)
    if (!model) continue
    models[record.id] = model
    if (record.service_tiers?.includes("priority")) {
      models[`${record.id}-fast`] = {
        ...model,
        id: record.id,
        name: `${model.name} Fast`,
        options: { service_tier: "priority" },
      }
    }
  }
  return models
}

function cacheKeys(key, base) {
  const salt = createHash("sha256").update(CACHE_DOMAIN).digest()
  const input = Buffer.from(key, "utf8")
  return {
    encryption: Buffer.from(hkdfSync("sha256", input, salt, Buffer.from(`encryption\0${base}`), 32)),
    identity: Buffer.from(hkdfSync("sha256", input, salt, Buffer.from(`identity\0${base}`), 32)),
  }
}

function cacheIdentity(identityKey, base) {
  return createHmac("sha256", identityKey).update(`${CACHE_DOMAIN}\0${base}`).digest("base64url")
}

function cacheAad(envelope, base) {
  return Buffer.from(JSON.stringify([
    CACHE_DOMAIN,
    base,
    envelope.schema,
    envelope.algorithm,
    envelope.identity,
    envelope.fetched_at,
    envelope.fresh_until,
    envelope.stale_until,
  ]))
}

function secureEqual(left, right) {
  const a = Buffer.from(left)
  const b = Buffer.from(right)
  return a.length === b.length && timingSafeEqual(a, b)
}

function writeAtomic0600(target, content) {
  const directory = path.dirname(target)
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 })
  if (process.platform !== "win32") fs.chmodSync(directory, 0o700)
  const temporary = path.join(directory, `.${path.basename(target)}.${process.pid}.${randomBytes(8).toString("hex")}.tmp`)
  let descriptor
  try {
    descriptor = fs.openSync(temporary, "wx", 0o600)
    fs.writeFileSync(descriptor, content, "utf8")
    fs.fsyncSync(descriptor)
    fs.closeSync(descriptor)
    descriptor = undefined
    fs.renameSync(temporary, target)
    if (process.platform !== "win32") fs.chmodSync(target, 0o600)
  } catch (error) {
    if (descriptor !== undefined) fs.closeSync(descriptor)
    try {
      fs.unlinkSync(temporary)
    } catch {}
    throw error
  }
}

function writeCapabilityCache({ cachePath, key, base, records, now = Date.now() }) {
  const normalizedBase = normalizeBase(base)
  const normalizedRecords = records.map(validateRecord)
  const payload = Buffer.from(JSON.stringify({ schema: CACHE_SCHEMA, models: normalizedRecords }))
  const keys = cacheKeys(key, normalizedBase)
  const envelope = {
    schema: CACHE_SCHEMA,
    algorithm: CACHE_ALGORITHM,
    identity: cacheIdentity(keys.identity, normalizedBase),
    fetched_at: now,
    fresh_until: now + CACHE_FRESH_TTL_MS,
    stale_until: now + CACHE_MAX_STALE_MS,
  }
  const iv = randomBytes(12)
  const cipher = createCipheriv(CACHE_ALGORITHM, keys.encryption, iv)
  cipher.setAAD(cacheAad(envelope, normalizedBase))
  const ciphertext = Buffer.concat([cipher.update(payload), cipher.final()])
  Object.assign(envelope, {
    iv: iv.toString("base64url"),
    tag: cipher.getAuthTag().toString("base64url"),
    ciphertext: ciphertext.toString("base64url"),
  })
  writeAtomic0600(cachePath, `${JSON.stringify(envelope)}\n`)
}

function readCapabilityCache({ cachePath, key, base, now = Date.now() }) {
  const stat = fs.lstatSync(cachePath)
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("capability cache is not a regular file")
  if (process.platform !== "win32" && (stat.mode & 0o077) !== 0) {
    throw new Error("capability cache permissions are not 0600")
  }
  const envelope = assertPlainObject(JSON.parse(fs.readFileSync(cachePath, "utf8")), "cache envelope")
  assertExactKeys(envelope, CACHE_ENVELOPE_KEYS, "cache envelope")
  if (envelope.schema !== CACHE_SCHEMA || envelope.algorithm !== CACHE_ALGORITHM) {
    throw new Error("unsupported capability cache version")
  }
  for (const field of ["fetched_at", "fresh_until", "stale_until"]) {
    if (!Number.isSafeInteger(envelope[field]) || envelope[field] < 0) throw new Error(`invalid cache ${field}`)
  }
  if (envelope.fresh_until !== envelope.fetched_at + CACHE_FRESH_TTL_MS
    || envelope.stale_until !== envelope.fetched_at + CACHE_MAX_STALE_MS) {
    throw new Error("invalid capability cache lifetime")
  }
  if (envelope.fetched_at > now + CACHE_CLOCK_SKEW_MS) throw new Error("capability cache is from the future")
  if (now > envelope.stale_until) throw new Error("capability cache expired")

  const normalizedBase = normalizeBase(base)
  const keys = cacheKeys(key, normalizedBase)
  const expectedIdentity = cacheIdentity(keys.identity, normalizedBase)
  if (typeof envelope.identity !== "string" || !secureEqual(envelope.identity, expectedIdentity)) {
    throw new Error("capability cache identity mismatch")
  }
  try {
    const decipher = createDecipheriv(CACHE_ALGORITHM, keys.encryption, Buffer.from(envelope.iv, "base64url"))
    decipher.setAAD(cacheAad(envelope, normalizedBase))
    decipher.setAuthTag(Buffer.from(envelope.tag, "base64url"))
    const plaintext = Buffer.concat([
      decipher.update(Buffer.from(envelope.ciphertext, "base64url")),
      decipher.final(),
    ])
    const payload = assertPlainObject(JSON.parse(plaintext.toString("utf8")), "cache payload")
    assertExactKeys(payload, new Set(["schema", "models"]), "cache payload")
    if (payload.schema !== CACHE_SCHEMA || !Array.isArray(payload.models)) {
      throw new Error("invalid capability cache payload")
    }
    const records = payload.models.map(validateRecord)
    if (new Set(records.map((record) => record.id)).size !== records.length) {
      throw new Error("duplicate cached model id")
    }
    return { records, fetchedAt: envelope.fetched_at, freshUntil: envelope.fresh_until }
  } catch (error) {
    throw new Error(`capability cache authentication failed: ${error.message}`)
  }
}

async function fetchCatalog(key, base, fetchImpl = fetch) {
  const response = await fetchImpl(`${normalizeBase(base)}/models`, {
    headers: { Authorization: `Bearer ${key}` },
    signal: AbortSignal.timeout(10000),
  })
  if (!response.ok) throw new Error(`catalog ${response.status}`)
  const body = assertPlainObject(await response.json(), "catalog response")
  if (!Array.isArray(body.data)) throw new Error("catalog response has no data array")
  const entries = body.data
  const records = entries.map(capabilityRecord)
  if (new Set(records.map((record) => record.id)).size !== records.length) {
    throw new Error("duplicate catalog model id")
  }
  return { models: liveModels(entries, records), records }
}

async function discoverModels({
  key,
  base,
  cachePath = defaultCachePath(),
  fetchImpl = fetch,
  now = Date.now(),
  warn = (message) => console.warn(message),
}) {
  try {
    const live = await fetchCatalog(key, base, fetchImpl)
    try {
      writeCapabilityCache({ cachePath, key, base, records: live.records, now })
    } catch (error) {
      warn(`[apitoken-router] capability cache write failed: ${error.message}`)
    }
    return { models: live.models, source: "live" }
  } catch (liveError) {
    try {
      const cached = readCapabilityCache({ cachePath, key, base, now })
      warn(`[apitoken-router] live catalog failed (${liveError.message}); using stale capability metadata from ${new Date(cached.fetchedAt).toISOString()} without pricing`)
      return { models: cachedModels(cached.records), source: "stale" }
    } catch (cacheError) {
      warn(`[apitoken-router] catalog unavailable (${liveError.message}); capability cache rejected (${cacheError.message})`)
      return { models: {}, source: "none" }
    }
  }
}

export default async function ApitokenRouter() {
  const { key, base } = readConnection()
  let models = {}
  if (key) ({ models } = await discoverModels({ key, base }))
  return {
    config: (cfg) => {
      cfg.provider ??= {}
      cfg.provider.apitoken ??= {}
      cfg.provider.apitoken.options ??= {}
      if (Object.keys(models).length === 0) return
      cfg.provider.apitoken.models = { ...models, ...(cfg.provider.apitoken.models ?? {}) }
    },
  }
}

// OpenCode treats every ESM export as a plugin factory. Keep the module's public export surface to
// the single default function; deterministic tests use this non-enumerable property instead.
Object.defineProperty(ApitokenRouter, "testing", {
  value: Object.freeze({
    CACHE_FRESH_TTL_MS,
    CACHE_MAX_STALE_MS,
    defaultCachePath,
    discoverModels,
    readCapabilityCache,
    readConnection,
  }),
})
