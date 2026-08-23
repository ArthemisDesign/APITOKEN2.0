#!/usr/bin/env python3
import argparse, hashlib, json, os, pathlib, subprocess, time

def sha(s):
    if len(s)!=40 or any(c not in '0123456789abcdef' for c in s): raise SystemExit('invalid sha')
    return s
p=argparse.ArgumentParser(); p.add_argument('--mode',choices=['promotion','hotfix'],required=True); p.add_argument('--commit',type=sha,required=True); p.add_argument('--actor',required=True); p.add_argument('--reason',required=True); p.add_argument('--state-root',default='/var/lib/apitoken-staging/watchdog'); p.add_argument('--repo',default='/opt/apitoken-staging/repo'); p.add_argument('--now',type=int,default=int(time.time())); a=p.parse_args()
if not (1 <= len(a.actor) <= 128 and 1 <= len(a.reason) <= 512): raise SystemExit('invalid audit fields')
root=pathlib.Path(a.state_root); marker=(root/'deployed.sha').read_text().strip()
if marker != a.commit: raise SystemExit('commit is not deployed stage marker')
tree=subprocess.check_output(['git','-c',f'safe.directory={a.repo}','-C',a.repo,'rev-parse',f'{a.commit}^{{tree}}'],text=True).strip()
policy_path=pathlib.Path(os.environ.get('STAGE_POLICY_FILE','deploy/stage-degradation-policy.json'))
policy=policy_path.read_bytes()
record={'mode':a.mode,'unix_user':'deploy','github_actor':a.actor,'commit_sha':a.commit,'tree_sha':tree,'artifact_digests':{},'policy_digest':hashlib.sha256(policy).hexdigest(),'contour_id':'stage','issued_at':a.now,'expires_at':a.now+86400,'reason':a.reason,'candidate_marker':marker}
payload=json.dumps(record,sort_keys=True,separators=(',',':'))
record['record_digest']=hashlib.sha256(payload.encode()).hexdigest()
out=root/'promotion-approved.json'; tmp=root/'.promotion-approved.tmp'; tmp.write_text(json.dumps(record,sort_keys=True)+'\n'); os.chmod(tmp,0o600); os.replace(tmp,out)
print(json.dumps(record,sort_keys=True))
