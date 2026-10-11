import http.server,threading,json,os,subprocess,pathlib,tempfile,re,hashlib,sys
binary=pathlib.Path(sys.argv[1]).resolve();root=pathlib.Path(tempfile.mkdtemp(prefix='hook-quiet-demo-'));home=root/'home';home.mkdir();work=root/'work';work.mkdir();(work/'.bobbin').mkdir();state=root/'state';state.mkdir();mode={'value':'empty'};requests=[]
class Server(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  data=json.loads(self.rfile.read(int(self.headers.get('Content-Length',0))) or '{}');query=data.get('query','');requests.append({'path':self.path,'mode':mode['value'],'query':query})
  if self.path=='/knot':self.send_response(401);self.end_headers();self.wfile.write(b'fixture auth refusal');return
  rows=[]
  if mode['value']=='warn':
   iri={'type':'uri','value':'http://example.org/fixture1'}
   if 'commandSignature' in query:rows=[{'s':iri}]
   elif 'a ?t' in query:rows=[{'s':iri,'t':{'type':'uri','value':'http://example.org/ontology/CommandDiskImpactObservation'}}]
   elif 'filesystemIdentity' in query:
    files=list((state/'yupana/disk').glob('*.json'));fs=json.loads(files[-1].read_text())['filesystem'];rows=[{'s':iri,'fs':{'type':'literal','value':fs}}]
   elif 'diskDeltaBytes' in query:rows=[{'s':iri,'delta':{'type':'literal','value':'1000000'}}]
  body=json.dumps({'head':{'vars':[]},'results':{'bindings':rows}}).encode();self.send_response(200);self.send_header('Content-Type','application/sparql-results+json');self.end_headers();self.wfile.write(body)
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Server);threading.Thread(target=server.serve_forever,daemon=True).start();port=server.server_port
(work/'.bobbin/config.toml').write_text(f'[yupana.quipu]\nenabled=true\nendpoint="http://127.0.0.1:{port}"\n')
env={'PATH':'/usr/bin:/bin','HOME':str(home),'XDG_CONFIG_HOME':str(home/'.config'),'XDG_STATE_HOME':str(state),'YUPANA_DISK_STATE_DIR':str(state),'YUPANA_METRICS_PATH':str(root/'metrics.jsonl'),'YUPANA_FAILOPEN_MARKER_DIR':str(root/'markers'),'YUPANA_DISK_HEADROOM_BYTES':'1','SHANTY_AGENT':'qddvr6-private-probe','QUIPU_AUTH_TOKEN':'fixture-only-not-a-real-secret'}
results=[]
for name,kind in [('unknown-no-history','empty'),('unknown-failed-write','empty'),('positive-governed-warn','warn')]:
 mode['value']=kind;payload=json.dumps({'session_id':'qddvr6-private','cwd':str(work),'tool_use_id':name,'tool_name':'Bash','tool_input':{'command':'cargo build'}})
 r=subprocess.run([str(binary),'hook','pre-bash'],input=payload,text=True,env=env,cwd=work,capture_output=True,timeout=30);results.append({'name':name,'rc':r.returncode,'stdout':r.stdout,'stderr':r.stderr})
server.shutdown()
events=[json.loads(x) for x in (root/'metrics.jsonl').read_text().splitlines()]
for r in results:
 print(json.dumps({'case':r['name'],'exit':r['rc'],'stdout_bytes':len(r['stdout'].encode()),'stderr_lines':len(r['stderr'].splitlines()),'warn_visible':'disk history predicts p90' in r['stdout']}))
print(json.dumps({'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'unknown_reason_events':sum(e['kind']=='disk_guard_unknown' and bool(e.get('reason')) for e in events),'auth_failure_events':sum(e['kind']=='quipu_auth_failure' for e in events)}))
assert all(r['rc']==0 for r in results)
assert not results[0]['stdout'] and not results[0]['stderr']
assert 'disk history predicts p90' in results[2]['stdout']
import shutil
shutil.rmtree(root)
