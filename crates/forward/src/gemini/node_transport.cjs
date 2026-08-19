'use strict';

// Minimal, dependency-free transport companion for the reviewed Cloud Code network profile.
// The Rust parent attests the exact Node executable before this source is evaluated. Requests use
// Node's native HTTPS/TLS implementation; no browser/BoringSSL impersonation options are applied.
const http = require('node:http');
const https = require('node:https');
const net = require('node:net');
const tls = require('node:tls');
const zlib = require('node:zlib');
const readline = require('node:readline');
const {Readable} = require('node:stream');
// The executable SHA pin makes this internal module a stable, attested part of the selected Node
// runtime. It is the same dispatcher implementation consumed by Node's global fetch; using it
// avoids substituting the gaxios HTTPS profile for Gemini CLI's distinct userinfo fetch profile.
const undici = require('internal/deps/undici/undici');

const PROTOCOL = 1;
const MAX_REQUEST_BYTES = 32 * 1024 * 1024;
const MAX_LINE_CHARS = 48 * 1024 * 1024;
// Liveness comes from TCP probes, not from a wall-clock deadline. A rebooted proxy, a dead peer or
// an expired NAT mapping is indistinguishable from a model that is still thinking if the only
// signal we watch is silence on the socket — which is exactly why the read timeout had to double
// as a client-facing limit. Probes answer that question directly: a dead peer resets within about
// a minute, while a healthy long request keeps answering them for as long as it needs. 60s matches
// every other plane (upstream.rs, glm/client.rs, kimi/client.rs). Behind a CONNECT proxy the probes
// travel only between us and the proxy, so the provider never observes them.
const KEEPALIVE_MS = 60000;
// Upper bound for a per-request silence allowance. Deliberately far above any plausible generation:
// it exists so a malformed frame cannot disable the backstop entirely, not to cap customers.
const MAX_READ_TIMEOUT_MS = 3600000;
let configured = false;
let proxyAgent;
let directAgent;
let connectTimeoutMs = 30000;
let readTimeoutMs = 120000;
const active = new Map();
const responseStreams = new Set();
let stdoutBlocked = false;

function emit(frame) {
  const writable = process.stdout.write(`${JSON.stringify(frame)}\n`);
  if (!writable && !stdoutBlocked) {
    stdoutBlocked = true;
    for (const stream of responseStreams) stream.pause();
  }
  return writable;
}

process.stdout.on('drain', () => {
  stdoutBlocked = false;
  for (const stream of responseStreams) stream.resume();
});

function fail(id, kind) {
  emit({type: 'error', id, kind});
}

function requestFailureKind(error) {
  const message = error && typeof error.message === 'string' ? error.message : '';
  if (message === 'timeout') return 'timeout';
  if (message === 'proxy-timeout') return 'proxy-timeout';
  if (message === 'proxy-auth') return 'proxy-auth';
  if (message === 'proxy-throttle') return 'proxy-throttle';
  if (message === 'proxy-rejected') return 'proxy-rejected';
  if (message === 'proxy-upstream') return 'proxy-upstream';
  if (message === 'proxy-connect') return 'proxy-connect';
  if (message === 'proxy-eof') return 'proxy-eof';
  if (message === 'proxy-protocol') return 'proxy-protocol';
  if (message === 'tls') return 'tls';
  if (message === 'calibration-expired') return 'calibration-expired';
  if (message === 'protocol') return 'protocol';
  return 'network';
}

function boundedInteger(value, minimum, maximum) {
  return Number.isInteger(value) && value >= minimum && value <= maximum;
}

function calibrationAttestation(notAfter) {
  if (notAfter === undefined) return undefined;
  if (!boundedInteger(notAfter, 1, Math.floor(Number.MAX_SAFE_INTEGER / 1000))) {
    throw new Error('request');
  }
  return {notAfterMs: notAfter * 1000, dispatchMs: undefined};
}

// Equality is expired: the controlled exact-profile caller grants [now, notAfter),
// never the boundary itself. Early checks deliberately do not write the attestation; only the
// pinned ClientRequest `socket` event immediately before `_flush` may create the outward dispatch
// proof.
function checkCalibrationDeadline(attestation) {
  if (!attestation) return;
  const nowMs = Date.now();
  if (!Number.isSafeInteger(nowMs) || nowMs <= 0 || nowMs >= attestation.notAfterMs) {
    throw new Error('calibration-expired');
  }
}

function recordCalibrationDispatch(attestation) {
  if (!attestation) return;
  if (attestation.dispatchMs !== undefined) throw new Error('protocol');
  checkCalibrationDeadline(attestation);
  const dispatchMs = Date.now();
  if (!Number.isSafeInteger(dispatchMs) || dispatchMs <= 0 || dispatchMs >= attestation.notAfterMs) {
    throw new Error('calibration-expired');
  }
  attestation.dispatchMs = dispatchMs;
}

function proxyOptions(proxy) {
  if (typeof proxy !== 'string' || proxy.length < 8 || proxy.length > 4096) {
    throw new Error('proxy');
  }
  const parsed = new URL(proxy);
  if ((parsed.protocol !== 'http:' && parsed.protocol !== 'https:') ||
      !parsed.hostname || (parsed.pathname !== '' && parsed.pathname !== '/') ||
      parsed.search || parsed.hash) {
    throw new Error('proxy');
  }
  // https-proxy-agent decodes URL credentials before constructing Basic auth. Do it while the
  // configure frame is still inside a synchronous try/catch: decodeURIComponent in the later
  // socket callback would otherwise turn malformed percent encoding into an uncaught exception
  // that kills every multiplexed request in this profile helper.
  let authorization;
  if (parsed.username || parsed.password) {
    const auth = `${decodeURIComponent(parsed.username)}:${decodeURIComponent(parsed.password)}`;
    authorization = `Basic ${Buffer.from(auth).toString('base64')}`;
  }
  return {url: parsed, authorization};
}

function connectStatusFailure(status) {
  if (status === 407) return new Error('proxy-auth');
  // Residential gateways commonly use 403 as a short-lived connection/concurrency throttle.
  // It must not be reported as invalid credentials: the same allocation can recover unchanged.
  if (status === 403 || status === 429) return new Error('proxy-throttle');
  if (status >= 500 && status <= 599) return new Error('proxy-upstream');
  return new Error('proxy-rejected');
}

function readConnectResponse(socket, callback) {
  let buffered = Buffer.alloc(0);
  const cleanup = () => {
    socket.off('data', onData);
    socket.off('error', onError);
    socket.off('end', onEnd);
  };
  const onError = () => { cleanup(); callback(new Error('proxy-connect')); };
  const onEnd = () => { cleanup(); callback(new Error('proxy-eof')); };
  const onData = chunk => {
    buffered = Buffer.concat([buffered, chunk]);
    if (buffered.length > 64 * 1024) {
      cleanup();
      callback(new Error('proxy-protocol'));
      return;
    }
    const boundary = buffered.indexOf('\r\n\r\n');
    if (boundary === -1) return;
    cleanup();
    const head = buffered.subarray(0, boundary).toString('latin1');
    const first = head.split('\r\n', 1)[0] || '';
    const match = /^HTTP\/1\.[01] ([0-9]{3})(?: |$)/.exec(first);
    if (!match) {
      callback(new Error('proxy-protocol'));
      return;
    }
    const status = Number(match[1]);
    if (status !== 200) {
      callback(connectStatusFailure(status));
      return;
    }
    const remainder = buffered.subarray(boundary + 4);
    if (remainder.length) socket.unshift(remainder);
    callback(null);
  };
  socket.on('data', onData);
  socket.once('error', onError);
  socket.once('end', onEnd);
}

// Mirrors the CONNECT and target TLS behaviour of https-proxy-agent used by gaxios. The agent is
// deliberately not keep-alive: that is the official proxy-agent default. The persistent helper
// still owns one isolated agent/process per subscription and safely multiplexes concurrent calls.
class GeminiProxyAgent extends https.Agent {
  constructor(proxy) {
    super({keepAlive: false});
    const options = proxyOptions(proxy);
    this.proxy = options.url;
    this.authorization = options.authorization;
  }

  createConnection(options, callback) {
    try {
      // A frame can wait behind Rust mutex/spawn/IPC and the Node event loop. Re-check in the
      // helper synchronously before even opening the proxy socket.
      checkCalibrationDeadline(options.calibrationAttestation);
    } catch (error) {
      callback(error);
      return undefined;
    }
    const proxyPort = this.proxy.port ? Number(this.proxy.port) :
      (this.proxy.protocol === 'https:' ? 443 : 80);
    const raw = this.proxy.protocol === 'https:'
      ? tls.connect({
          host: this.proxy.hostname,
          port: proxyPort,
          servername: net.isIP(this.proxy.hostname) ? undefined : this.proxy.hostname,
          ALPNProtocols: ['http/1.1'],
        })
      : net.connect({host: this.proxy.hostname, port: proxyPort});
    let settled = false;
    const finish = (error, socket) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) raw.destroy();
      callback(error, socket);
    };
    const timer = setTimeout(() => finish(new Error('proxy-timeout')), connectTimeoutMs);
    timer.unref();
    raw.once('error', () => finish(new Error('proxy-connect')));
    const connectedEvent = this.proxy.protocol === 'https:' ? 'secureConnect' : 'connect';
    raw.once(connectedEvent, () => {
      const targetHost = net.isIPv6(options.host) ? `[${options.host}]` : options.host;
      let payload = `CONNECT ${targetHost}:${options.port} HTTP/1.1\r\n`;
      if (this.authorization) payload += `Proxy-Authorization: ${this.authorization}\r\n`;
      payload += `Host: ${targetHost}:${options.port}\r\nProxy-Connection: close\r\n\r\n`;
      readConnectResponse(raw, error => {
        if (error) {
          finish(error);
          return;
        }
        try {
          // CONNECT established only a tunnel. The provider has not seen the HTTP request yet;
          // reject an expired calibration before the target TLS socket is created.
          checkCalibrationDeadline(options.calibrationAttestation);
        } catch (deadlineError) {
          finish(deadlineError);
          return;
        }
        const socket = tls.connect({
          socket: raw,
          servername: options.servername || (net.isIP(options.host) ? undefined : options.host),
          rejectUnauthorized: options.rejectUnauthorized,
          ca: options.ca,
          cert: options.cert,
          key: options.key,
          ciphers: options.ciphers,
          minVersion: options.minVersion,
          maxVersion: options.maxVersion,
        });
        socket.once('secureConnect', () => {
          try {
            // Keep the half-open fence valid while handing the target TLS socket to ClientRequest.
            // The pinned Node pre-flush `socket` event below records the later dispatch proof.
            checkCalibrationDeadline(options.calibrationAttestation);
            finish(null, socket);
          } catch (deadlineError) {
            socket.destroy();
            finish(deadlineError);
          }
        });
        socket.once('error', () => finish(new Error('tls')));
      });
      raw.write(payload);
    });
    return undefined;
  }
}

// Production profiles require a proxy, but literal direct HTTPS remains a reviewed helper mode.
// Keep it under the same final TLS handoff proof instead of falling back to the global Agent.
class GeminiDirectAgent extends https.Agent {
  constructor() {
    super({keepAlive: false});
  }

  createConnection(options, callback) {
    let socket;
    try {
      checkCalibrationDeadline(options.calibrationAttestation);
      socket = tls.connect({
        host: options.host,
        port: options.port,
        servername: options.servername || (net.isIP(options.host) ? undefined : options.host),
        rejectUnauthorized: options.rejectUnauthorized,
        ca: options.ca,
        cert: options.cert,
        key: options.key,
        ciphers: options.ciphers,
        minVersion: options.minVersion,
        maxVersion: options.maxVersion,
        ALPNProtocols: ['http/1.1'],
      });
    } catch (error) {
      callback(error);
      return undefined;
    }
    let settled = false;
    const finish = (error, connected) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) socket.destroy();
      callback(error, connected);
    };
    const timer = setTimeout(() => finish(new Error('timeout')), connectTimeoutMs);
    timer.unref();
    socket.once('secureConnect', () => {
      try {
        checkCalibrationDeadline(options.calibrationAttestation);
        finish(null, socket);
      } catch (deadlineError) {
        finish(deadlineError);
      }
    });
    socket.once('error', () => finish(new Error('tls')));
    return undefined;
  }
}

function responseStream(response) {
  const encoding = String(response.headers['content-encoding'] || '').toLowerCase().trim();
  let decompressor;
  if (encoding === 'gzip' || encoding === 'x-gzip') decompressor = zlib.createGunzip();
  else if (encoding === 'deflate' || encoding === 'x-deflate') decompressor = zlib.createInflate();
  else if (encoding === 'br') decompressor = zlib.createBrotliDecompress();
  else return response;
  // Stream.pipe does not forward source errors and only unpipes on transform errors. Explicitly
  // couple both halves so a corrupt/truncated compressed response cannot leave the original
  // IncomingMessage consuming a socket after Rust has already classified the request as failed.
  response.once('error', error => decompressor.destroy(error));
  decompressor.once('error', () => response.destroy());
  return response.pipe(decompressor);
}

function normalizeHeaders(entries, bodyLength, method) {
  if (!Array.isArray(entries) || entries.length > 32) throw new Error('headers');
  const headers = new Map();
  for (const entry of entries) {
    if (!Array.isArray(entry) || entry.length !== 2 ||
        typeof entry[0] !== 'string' || typeof entry[1] !== 'string' ||
        entry[0].length === 0 || entry[0].length > 128 || entry[1].length > 16384) {
      throw new Error('headers');
    }
    http.validateHeaderName(entry[0]);
    http.validateHeaderValue(entry[0], entry[1]);
    headers.set(entry[0].toLowerCase(), entry[1]);
  }
  if (!headers.has('accept')) headers.set('accept', '*/*');
  if (!headers.has('accept-encoding')) headers.set('accept-encoding', 'gzip, deflate, br');
  if (method !== 'GET') headers.set('content-length', String(bodyLength));
  // node-fetch, used by the pinned Gemini CLI gaxios stack, sorts its Headers before handing them
  // to https.request. Preserve that observable HTTP/1.1 ordering.
  return Object.fromEntries([...headers.entries()].sort(([left], [right]) => left.localeCompare(right)));
}

function exactHeaders(entries) {
  if (!Array.isArray(entries) || entries.length > 32) throw new Error('headers');
  const headers = {};
  for (const entry of entries) {
    if (!Array.isArray(entry) || entry.length !== 2 ||
        typeof entry[0] !== 'string' || typeof entry[1] !== 'string' ||
        entry[0].length === 0 || entry[0].length > 128 || entry[1].length > 16384) {
      throw new Error('headers');
    }
    http.validateHeaderName(entry[0]);
    http.validateHeaderValue(entry[0], entry[1]);
    headers[entry[0]] = entry[1];
  }
  return headers;
}

// Gemini CLI's fetchAndCacheUserInfo deliberately uses global fetch, not OAuth2Client.request.
// That means a different observable profile: Undici CONNECT pooling, its TLS ClientHello, and
// fetch defaults such as user-agent "node" and sec-fetch-mode. Keep this path narrow so Code
// Assist generation/token calls cannot accidentally drift away from their gaxios profile.
function startUndiciFetch(frame) {
  const id = frame.id;
  try {
    if (frame.calibrationNotAfter !== undefined || frame.method !== 'GET' ||
        typeof frame.url !== 'string' || frame.url.length > 8192 ||
        typeof frame.body !== 'string') throw new Error('request');
    const url = new URL(frame.url);
    if (url.protocol !== 'https:') throw new Error('scheme');
    const body = Buffer.from(frame.body, 'base64');
    if (body.length !== 0 || body.toString('base64') !== frame.body) throw new Error('body');
    const headers = exactHeaders(frame.headers);
    const controller = new AbortController();
    active.set(id, {destroy: reason => controller.abort(reason)});
    fetch(url, {method: 'GET', headers, signal: controller.signal}).then(response => {
      if (!active.has(id)) return;
      const rawHeaders = [];
      for (const [name, value] of response.headers) rawHeaders.push(name, value);
      emit({type: 'headers', id, status: response.status, headers: rawHeaders.slice(0, 512)});
      if (!response.body) {
        active.delete(id);
        emit({type: 'end', id});
        return;
      }
      const stream = Readable.fromWeb(response.body);
      active.set(id, {
        destroy: reason => {
          controller.abort(reason);
          stream.destroy(reason);
        },
      });
      responseStreams.add(stream);
      if (stdoutBlocked) stream.pause();
      stream.on('data', chunk => emit({type: 'data', id, data: Buffer.from(chunk).toString('base64')}));
      stream.once('end', () => {
        responseStreams.delete(stream);
        if (!active.delete(id)) return;
        emit({type: 'end', id});
      });
      stream.once('error', () => {
        responseStreams.delete(stream);
        if (!active.delete(id)) return;
        fail(id, 'network');
      });
    }).catch(() => {
      if (!active.delete(id)) return;
      fail(id, 'network');
    });
  } catch (_) {
    active.delete(id);
    fail(id, 'protocol');
  }
}

function startRequest(frame) {
  const id = frame.id;
  if (!Number.isSafeInteger(id) || id <= 0 || active.has(id)) return;
  if (frame.wireProfile === 'undici-fetch') {
    startUndiciFetch(frame);
    return;
  }
  try {
    if (frame.wireProfile !== undefined ||
        (frame.method !== 'POST' && frame.method !== 'GET') ||
        typeof frame.url !== 'string' || frame.url.length > 8192 ||
        typeof frame.body !== 'string') throw new Error('request');
    // Absent means the process-wide value from the configure frame; an explicit 0 means no
    // deadline, which is what customer generation sends — liveness there comes from the keepalive
    // probes below, and any wall-clock value would just be a bet on how long a model may think.
    // A present-but-invalid value fails the request rather than being coerced, so a caller bug
    // surfaces instead of quietly reinstating a ceiling.
    if (frame.readTimeoutMs !== undefined && frame.readTimeoutMs !== 0 &&
        !boundedInteger(frame.readTimeoutMs, 1000, MAX_READ_TIMEOUT_MS)) throw new Error('request');
    const idleTimeoutMs = frame.readTimeoutMs === undefined ? readTimeoutMs : frame.readTimeoutMs;
    const url = new URL(frame.url);
    if (url.protocol !== 'https:') throw new Error('scheme');
    const body = Buffer.from(frame.body, 'base64');
    if (body.length > MAX_REQUEST_BYTES || body.toString('base64') !== frame.body) {
      throw new Error('body');
    }
    if (frame.method === 'GET' && body.length !== 0) throw new Error('body');
    const headers = normalizeHeaders(frame.headers, body.length, frame.method);
    const calibration = calibrationAttestation(frame.calibrationNotAfter);
    // Re-check after all synchronous decode/validation work and immediately before https.request.
    // This catches a frame that expired in the IPC/readline queue without touching any socket.
    checkCalibrationDeadline(calibration);
    const request = https.request(url, {
      method: frame.method,
      headers,
      // Preserve the ordinary direct profile on Node's globalAgent. The private direct agent
      // exists only to carry deadline metadata through createConnection; proxy traffic already
      // used the dedicated profile agent before this fence was introduced.
      agent: proxyAgent || (calibration ? directAgent : undefined),
      maxHeaderSize: 64 * 1024,
      calibrationAttestation: calibration,
    }, response => {
      if (!active.has(id)) {
        response.destroy();
        return;
      }
      const stream = responseStream(response);
      active.set(id, {
        destroy: reason => {
          request.destroy(reason);
          response.destroy(reason);
          if (stream !== response) stream.destroy(reason);
        },
      });
      responseStreams.add(stream);
      if (stdoutBlocked) stream.pause();
      const rawHeaders = response.rawHeaders.slice(0, 512);
      const head = {type: 'headers', id, status: response.statusCode || 0, headers: rawHeaders};
      if (calibration) {
        if (!Number.isSafeInteger(calibration.dispatchMs) || calibration.dispatchMs <= 0 ||
            calibration.dispatchMs >= calibration.notAfterMs) {
          // Response headers prove the POST started. Missing final attestation here is helper
          // protocol corruption, never a pre-dispatch expiry/not-started proof.
          request.destroy(new Error('protocol'));
          response.destroy();
          return;
        }
        head.calibrationDispatchMs = calibration.dispatchMs;
      }
      emit(head);
      stream.on('data', chunk => emit({type: 'data', id, data: Buffer.from(chunk).toString('base64')}));
      stream.once('end', () => {
        responseStreams.delete(stream);
        if (!active.delete(id)) return;
        emit({type: 'end', id});
      });
      stream.once('error', () => {
        responseStreams.delete(stream);
        if (!active.delete(id)) return;
        fail(id, 'network');
      });
    });
    active.set(id, request);
    // Exact pinned Node v24.18.0 `_http_client` runs `req.emit('socket', socket)` synchronously in
    // `tickOnSocket`, then calls `req._flush()`. Record last in that listener: no helper work sits
    // between the timestamp and returning to the exact pre-write core boundary. Destroying here
    // leaves `_flush` a destroyed request and emits no HTTP bytes.
    const onSocket = socket => {
      try {
        socket.setKeepAlive(true, KEEPALIVE_MS);
        recordCalibrationDispatch(calibration);
        if (frame.observeActualSend === true) {
          emit({type: 'actual_send', id, actualSend: true});
        } else if (frame.observeActualSend !== undefined) {
          throw new Error('protocol');
        }
      } catch (error) {
        request.destroy(error);
      }
    };
    // startRequest stays in one turn and both private agents disable reuse. ClientRequest.onSocket
    // schedules the pinned onSocketNT on nextTick, so registering now always precedes the event.
    request.once('socket', onSocket);
    if (idleTimeoutMs !== 0) {
      request.setTimeout(idleTimeoutMs, () => request.destroy(new Error('timeout')));
    }
    request.once('error', error => {
      if (!active.delete(id)) return;
      fail(id, requestFailureKind(error));
    });
    request.end(body.length ? body : undefined);
  } catch (error) {
    active.delete(id);
    fail(id, error && error.message === 'calibration-expired' ? 'calibration-expired' : 'protocol');
  }
}

const lines = readline.createInterface({input: process.stdin, crlfDelay: Infinity});
lines.on('line', line => {
  if (line.length > MAX_LINE_CHARS) process.exit(70);
  let frame;
  try { frame = JSON.parse(line); } catch (_) { process.exit(70); }
  if (!configured) {
    if (!frame || frame.type !== 'configure' || frame.protocol !== PROTOCOL ||
        !boundedInteger(frame.connectTimeoutMs, 1000, 120000) ||
        !boundedInteger(frame.readTimeoutMs, 1000, 600000)) process.exit(70);
    try {
      proxyAgent = frame.proxy ? new GeminiProxyAgent(frame.proxy) : undefined;
      directAgent = frame.proxy ? undefined : new GeminiDirectAgent();
      if (frame.proxy) {
        // Exact defaults from Gemini CLI 0.53.0 setGlobalProxy(). env_clear plus an explicit empty
        // noProxy prevent another subscription or host environment from bypassing this profile.
        undici.setGlobalDispatcher(new undici.EnvHttpProxyAgent({
          httpProxy: frame.proxy,
          httpsProxy: frame.proxy,
          noProxy: '',
          headersTimeout: 60000,
          bodyTimeout: 300000,
        }));
      }
    } catch (_) {
      process.exit(70);
    }
    connectTimeoutMs = frame.connectTimeoutMs;
    readTimeoutMs = frame.readTimeoutMs;
    configured = true;
    emit({type: 'ready', protocol: PROTOCOL, node: process.version, platform: process.platform, arch: process.arch,
      undici: 'node-internal'});
    return;
  }
  if (frame && frame.type === 'request') startRequest(frame);
  else if (frame && frame.type === 'cancel' && Number.isSafeInteger(frame.id)) {
    const request = active.get(frame.id);
    // Delete first so the destroy-induced error/end callbacks stay silent. Rust intentionally
    // removed this pre-header request and would correctly treat a late frame as protocol drift.
    if (request && active.delete(frame.id)) request.destroy(new Error('cancelled'));
  } else process.exit(70);
});
lines.once('close', () => {
  for (const request of active.values()) request.destroy(new Error('closed'));
  process.exit(0);
});
