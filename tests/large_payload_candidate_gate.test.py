#!/usr/bin/env python3
import json,subprocess,tempfile,os
ROOT=os.path.dirname(__file__); gate=os.path.join(ROOT,'large_payload_candidate_gate.py')
def run(event_after=0,peak=100,spool=0,status=400):
 with tempfile.TemporaryDirectory() as d:
  before={'schema':'large-payload-cgroup-v1','unit':'claude-router@8801.service','memory':{'peak':50,'max':'1000','events':'oom:0,oom_kill:0,max:0'},'spool_files':0}
  after={'schema':'large-payload-cgroup-v1','unit':before['unit'],'memory':{'peak':peak,'max':'1000','events':f'oom:{event_after},oom_kill:0,max:0'},'spool_files':spool}
  load={'schema':'large-payload-load-v1','requests':[{'status':status}]}
  paths=[]
  for name,value in [('b',before),('a',after),('l',load)]:
   path=os.path.join(d,name);open(path,'w').write(json.dumps(value));paths.append(path)
  return subprocess.run([gate,'--sha','a'*40,'--before',paths[0],'--after',paths[1],'--load',paths[2],'--memory-high-bytes','900'],capture_output=True,text=True)
assert run().returncode==0
assert run(event_after=1).returncode==1
assert run(peak=850).returncode==1
assert run(spool=1).returncode==1
assert run(status=413).returncode==1
assert run(status=503).returncode==1
print('large-payload candidate gate tests passed')
