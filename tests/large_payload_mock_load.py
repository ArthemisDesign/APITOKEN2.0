#!/usr/bin/env python3
"""Deterministic, allocation-bounded large-payload load driver.

Bodies are generated while streaming; no giant fixture is stored in the repository or retained in
memory. The target must be a loopback mock/candidate URL. Results are content-free JSON evidence.

The body is valid JSON without `model`, so the router must admit it and then reject it locally.
A namespaced model would be forwarded to a provider plane whose own cap is still 8 or 32 MiB;
that plane's 413 is not router-admission evidence.
"""
import argparse, concurrent.futures, http.client, json, os, statistics, time, urllib.parse

CHUNK = 1024 * 1024
ALLOWED_HOSTS = {"127.0.0.1", "localhost", "::1"}

def send(url, size, chunked, timeout, authorization):
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "http" or parsed.hostname not in ALLOWED_HOSTS:
        raise ValueError("target must be an explicit loopback http URL")
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=timeout)
    path = parsed.path or "/"
    if parsed.query: path += "?" + parsed.query
    headers = {"content-type": "application/json", "authorization": authorization}
    started = time.monotonic()
    conn.putrequest("POST", path)
    for name, value in headers.items(): conn.putheader(name, value)
    if chunked: conn.putheader("transfer-encoding", "chunked")
    else: conn.putheader("content-length", str(size))
    conn.endheaders()
    remaining = size
    prefix = b'{"messages":[{"role":"user","content":"'
    suffix = b'"}]}'
    sent = 0
    for piece in (prefix,):
        data = piece[:remaining]; remaining -= len(data); sent += len(data)
        if chunked: conn.send(f"{len(data):X}\r\n".encode()+data+b"\r\n")
        else: conn.send(data)
    fill_remaining = max(0, remaining - len(suffix))
    block = b"x" * CHUNK
    while fill_remaining:
        data = block[:min(fill_remaining, CHUNK)]; fill_remaining -= len(data); remaining -= len(data); sent += len(data)
        if chunked: conn.send(f"{len(data):X}\r\n".encode()+data+b"\r\n")
        else: conn.send(data)
    data = suffix[:remaining]; sent += len(data)
    if chunked:
        conn.send(f"{len(data):X}\r\n".encode()+data+b"\r\n0\r\n\r\n")
    else: conn.send(data)
    response = conn.getresponse(); response.read(64 * 1024); status=response.status; conn.close()
    return {"bytes":sent,"chunked":chunked,"status":status,"latency_ms":round((time.monotonic()-started)*1000,3)}

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--sizes-mib", default="8,32,64,128,256")
    parser.add_argument("--concurrency", type=int, default=1)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--authorization-file", required=True)
    args=parser.parse_args()
    with open(args.authorization_file, encoding="utf-8") as credential:
        authorization=credential.read().strip()
    if not authorization or "\n" in authorization or "\r" in authorization or len(authorization)>512: raise SystemExit(2)
    sizes=[int(x)*1024*1024 for x in args.sizes_mib.split(",")]
    if not 1 <= args.concurrency <= 256 or any(x <= 0 or x > 256*1024*1024 for x in sizes): raise SystemExit(2)
    work=[(size, chunked) for size in sizes for chunked in (False, True)]
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        rows=list(pool.map(lambda item: send(args.url,item[0],item[1],args.timeout,authorization),work))
    lat=[r["latency_ms"] for r in rows]
    print(json.dumps({"schema":"large-payload-load-v1","target":urllib.parse.urlsplit(args.url).path,"concurrency":args.concurrency,
      "requests":rows,"latency_ms":{"max":max(lat),"median":statistics.median(lat)}},sort_keys=True,separators=(",",":")))
if __name__ == "__main__": main()
