import sys,time
f=open(sys.argv[1],'a',buffering=1)
for l in sys.stdin:
    if l.startswith('op '): f.write('%.3f %s'%(time.time(),l))
