#!/usr/bin/env python3
import json, os, time, urllib.request
base=os.environ.get('STAGE_BASE','http://10.254.32.2:3900')
lat=[]; errors=0
for i in range(20):
    started=time.monotonic()
    try:
        with urllib.request.urlopen(base+'/ready',timeout=2) as r:
            if r.status != 200: errors += 1
    except Exception: errors += 1
    lat.append(int((time.monotonic()-started)*1000))
lat.sort(); p95=lat[min(len(lat)-1,int(len(lat)*.95))]
print(json.dumps({'samples':len(lat),'errors':errors,'latency_p95_ms':p95},separators=(',',':')))
