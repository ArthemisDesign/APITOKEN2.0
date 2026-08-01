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
let configured = false;
let proxyAgent;
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
  return 'network';
}

function boundedInteger(value, minimum, maximum) {
  return Number.isInteger(value) && value >= minimum && value <= maximum;
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
        socket.once('secureConnect', () => finish(null, socket));
        socket.once('error', () => finish(new Error('tls')));
      });
      raw.write(payload);
    });
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
    if (frame.method !== 'GET' || typeof frame.url !== 'string' || frame.url.length > 8192 ||
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
    const url = new URL(frame.url);
    if (url.protocol !== 'https:') throw new Error('scheme');
    const body = Buffer.from(frame.body, 'base64');
    if (body.length > MAX_REQUEST_BYTES || body.toString('base64') !== frame.body) {
      throw new Error('body');
    }
    if (frame.method === 'GET' && body.length !== 0) throw new Error('body');
    const headers = normalizeHeaders(frame.headers, body.length, frame.method);
    const request = https.request(url, {
      method: frame.method,
      headers,
      agent: proxyAgent,
      maxHeaderSize: 64 * 1024,
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
      emit({type: 'headers', id, status: response.statusCode || 0, headers: rawHeaders});
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
    request.setTimeout(readTimeoutMs, () => request.destroy(new Error('timeout')));
    request.once('error', error => {
      if (!active.delete(id)) return;
      fail(id, requestFailureKind(error));
    });
    request.end(body.length ? body : undefined);
  } catch (_) {
    active.delete(id);
    fail(id, 'protocol');
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
