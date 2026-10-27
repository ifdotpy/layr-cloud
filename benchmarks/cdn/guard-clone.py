"""Run one clone with the lead-approved 12 GiB floor and minute-by-minute evidence."""
import datetime,json,os,pathlib,shutil,signal,subprocess,sys,time
host='/System/Volumes/Data';minimum=25*1024**3;floor=12*1024**3
evidence=pathlib.Path(sys.argv[1]);assert not evidence.exists()
def record(**fields):
 row={'observedUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'hostFreeBytes':shutil.disk_usage(host).free,'startMinimumBytes':minimum,'stopFloorBytes':floor,**fields}
 with evidence.open('a') as output:output.write(json.dumps(row)+'\n')
 return row
first=record(event='start');assert first['hostFreeBytes']>=minimum,'Clone not started: less than25GiB free'
subprocess.run(['df','-h',host],check=True)
p=subprocess.Popen(sys.argv[2:],start_new_session=True);next_record=time.monotonic()+60
while p.poll() is None:
 free=shutil.disk_usage(host).free
 if free<floor:
  record(event='stop',reason='below12GiB',signal='SIGTERM');os.killpg(p.pid,signal.SIGTERM);p.wait()
  print('Clone stopped: host free space below12GiB',flush=True);raise SystemExit(1)
 if time.monotonic()>=next_record:
  row=record(event='minute');next_record+=60
  print(json.dumps({'diskEvidenceUTC':row['observedUTC'],'hostFreeGiB':round(row['hostFreeBytes']/1024**3,2)}),flush=True)
 time.sleep(5)
record(event='exit',exitCode=p.returncode);raise SystemExit(p.returncode)
