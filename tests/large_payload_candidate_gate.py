#!/usr/bin/env python3
"""Evaluate exact-SHA large-payload evidence; emits no payload or secret data."""
import argparse,json,sys

def read(path):
 with open(path,encoding='utf-8') as f:return json.load(f)
def events(raw):
 return {k:int(v) for k,v in (entry.split(':',1) for entry in raw.split(',') if entry)}
def main():
 p=argparse.ArgumentParser();p.add_argument('--sha',required=True);p.add_argument('--before',required=True);p.add_argument('--after',required=True);p.add_argument('--load',required=True);p.add_argument('--memory-high-bytes',required=True,type=int);a=p.parse_args()
 if len(a.sha)!=40 or any(c not in '0123456789abcdef' for c in a.sha):raise SystemExit(2)
 before,after,load=read(a.before),read(a.after),read(a.load)
 if before.get('schema')!='large-payload-cgroup-v1' or after.get('schema')!='large-payload-cgroup-v1' or load.get('schema')!='large-payload-load-v1':raise SystemExit(2)
 if before['unit']!=after['unit']:raise SystemExit(2)
 be,ae=events(before['memory']['events']),events(after['memory']['events'])
 deltas={k:ae.get(k,0)-be.get(k,0) for k in set(be)|set(ae)}
 max_raw=after['memory']['max']; maximum=int(max_raw) if str(max_raw).isdigit() else 0
 peak=int(after['memory']['peak']); headroom_ok=maximum>0 and peak*100<=maximum*80
 statuses=[int(row.get('status',0)) for row in load['requests']]
 # Every body must cross raised router admission before the deliberately invalid model/credential
 # seam. Size/timeout/header rejection and transport/server failures cannot count as evidence.
 status_ok=bool(statuses) and all(400 <= status < 500 and status not in (408,413,431) for status in statuses)
 accepted=all(deltas.get(k,0)==0 for k in ('oom','oom_kill','max')) and after['spool_files']==0 and peak<a.memory_high_bytes and headroom_ok and status_ok
 result={'schema':'large-payload-acceptance-v1','sha':a.sha,'unit':after['unit'],'accepted':accepted,'memory_peak':peak,'memory_high':a.memory_high_bytes,'memory_max':maximum,'event_deltas':deltas,'spool_files':after['spool_files'],'requests':len(load['requests']),'statuses':statuses,'status_ok':status_ok}
 print(json.dumps(result,sort_keys=True,separators=(',',':')))
 return 0 if accepted else 1
if __name__=='__main__':sys.exit(main())
