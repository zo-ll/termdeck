import os,tty,pathlib
tty.setraw(0)
os.write(1,b"\x1b[?2004hREADY")
with open('/tmp/termdeck-audit-_1hmdxuj/input.bin',"wb",buffering=0) as f:
 while True:
  b=os.read(0,4096)
  if not b:break
  f.write(b)
