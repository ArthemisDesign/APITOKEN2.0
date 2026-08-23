#!/usr/bin/env python3
"""Host-side fixed reverse proxy. It exposes only production /v1/messages to stage veth."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import http.client
HOST="10.254.32.1"; PORT=9081; MAX_BODY=1_048_576
class Handler(BaseHTTPRequestHandler):
    def reply(self, code, body=b'{}'):
        self.send_response(code); self.send_header('content-type','application/json'); self.send_header('content-length',str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_GET(self): self.reply(200 if self.path=='/ready' else 404)
    def do_POST(self):
        if self.path!='/v1/messages': return self.reply(404)
        n=int(self.headers.get('content-length','0'))
        if n<2 or n>MAX_BODY: return self.reply(413)
        body=self.rfile.read(n)
        try:
            conn=http.client.HTTPConnection('127.0.0.1',8790,timeout=30)
            headers={'content-type':'application/json','x-api-key':self.headers.get('x-api-key',''),'anthropic-version':'2023-06-01'}
            conn.request('POST','/v1/messages',body,headers); r=conn.getresponse(); payload=r.read(MAX_BODY)
            self.send_response(r.status); self.send_header('content-type',r.getheader('content-type') or 'application/json'); self.send_header('content-length',str(len(payload))); self.end_headers(); self.wfile.write(payload)
        except Exception: self.reply(502)
    def log_message(self, fmt, *args): print(f'stage-live-host-proxy {fmt % args}',flush=True)
ThreadingHTTPServer((HOST,PORT),Handler).serve_forever()
