import os, pty, fcntl, termios, struct, subprocess, pathlib, time, socket, json, signal, threading
root=pathlib.Path('/tmp/termdeck-audit-_1hmdxuj'); runtime=root/'runtime'; runtime.mkdir(exist_ok=True)
child=root/'child.py'; capture=root/'input.bin'
child.write_text('import os,tty,pathlib\ntty.setraw(0)\nos.write(1,b"\\x1b[?2004hREADY")\nwith open('+repr(str(capture))+',"wb",buffering=0) as f:\n while True:\n  b=os.read(0,4096)\n  if not b:break\n  f.write(b)\n')
config=root/'live.json';config.write_text(json.dumps({'version':1,'defaults':{'command':['/bin/bash','--noprofile','--norc']},'workspaces':{'audit':{'root':str(root),'terminals':[{'name':'probe','cwd':'.','command':['/bin/bash','--noprofile','--norc']}]}}}))
master,slave=pty.openpty(); fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,100,0,0)); before=termios.tcgetattr(slave)
def child_setup():
 os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
env=os.environ.copy();env['XDG_RUNTIME_DIR']=str(runtime); env['HOME']=str(root);env.pop('TERMDECK_SOCK',None);env.pop('TERMDECK_PANE',None)
p=subprocess.Popen(['target/debug/termdeck',str(config)],stdin=slave,stdout=slave,stderr=slave,env=env,preexec_fn=child_setup)
output=bytearray(); stop=False
def drain():
 while not stop:
  try:
   data=os.read(master,65536)
   output.extend(data)
   if len(output)>1_000_000: del output[:-500_000]
  except OSError:break
threading.Thread(target=drain,daemon=True).start()
path=runtime/'termdeck'/str(p.pid)/'ctl.sock'
def call(**req):
 s=socket.socket(socket.AF_UNIX);s.settimeout(3);s.connect(str(path));s.sendall(json.dumps(dict(schema='ctl.v1',**req)).encode()+b'\n');data=b''
 while True:
  b=s.recv(65536)
  if not b:break
  data+=b
 s.close();return json.loads(data)
try:
 deadline=time.monotonic()+3
 while not path.exists() and time.monotonic()<deadline:time.sleep(.02)
 time.sleep(.1)
 time.sleep(.3)
 marker=root/'paste-executed'
 payload="\x1b[201~printf AUDIT > '"+str(marker)+"'\n"
 print('paste request:',call(verb='input',id='probe',paste=payload,force=True))
 time.sleep(.3)
 print('embedded paste closer executed command:',marker.exists())
 if marker.exists():print('marker contents:',marker.read_text())

finally:
 if p.poll() is None:
  p.send_signal(signal.SIGTERM)
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 stop=True;os.close(master);os.close(slave)
