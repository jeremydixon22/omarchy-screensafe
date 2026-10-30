"""Automatic preparation with guarded renderer feedback and bounded lifecycle."""
import hashlib
import json
import os
from pathlib import Path
import selectors
import re
import signal
import stat
import subprocess
import sys
import threading
import time
from types import SimpleNamespace
from progress import Display


def failure_path(args,source,size):
    code=[]
    for p in [Path(__file__),Path(__file__).with_name('manager.py'),args.renderer,args.worker]:
        s=p.stat();code.append([str(p),s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns])
    key=hashlib.sha256(json.dumps([source,size,code,os.uname().release],sort_keys=True).encode()).hexdigest()
    return args.root/('.auto-failure-'+key+'.json')


def recently_failed(path):
    try:
        fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        with os.fdopen(fd,'rb') as f:
            s=os.fstat(f.fileno())
            if not stat.S_ISREG(s.st_mode) or s.st_uid!=os.getuid() or s.st_mode&0o077:return False
            data=json.loads(f.read(1025))
        when=data.get('failedAt')
        return type(when) in [int,float] and 0<=time.time()-when<600
    except (OSError,ValueError,TypeError):return False


def record_failure(path):
    # Advisory retry suppression only; it cannot authorize or locate playback.
    try:
        entries=[p for p in path.parent.iterdir() if re.fullmatch(r'\.auto-failure-[a-f0-9]{64}\.json',p.name) and p.is_file() and not p.is_symlink()]
        for old in sorted(entries,key=lambda p:p.stat().st_mtime)[:-63]:
            if old!=path:old.unlink()
        fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_CLOEXEC|os.O_NOFOLLOW|os.O_NONBLOCK,0o600)
        with os.fdopen(fd,'w') as f:
            s=os.fstat(f.fileno())
            if not stat.S_ISREG(s.st_mode) or s.st_uid!=os.getuid() or s.st_mode&0o077 or s.st_nlink!=1:raise ValueError('invalid retry record')
            os.ftruncate(f.fileno(),0);json.dump({'failedAt':time.time()},f)
    except FileExistsError:pass


def notify(value):
    try:os.write(1,(json.dumps({'screensafePreparation':value},separators=(',',':'))+'\n').encode())
    except OSError:pass


def run(args,m):
    if not m.available(args):m.live(args)
    source=m.plan(args)
    if not source['eligible']:m.live(args)
    size=m.geometry()
    with m.store(args.root):
        failure=failure_path(args,source,size)
        if recently_failed(failure):m.live(args)
    command=m.arguments(args,'run')+['--preparation-ui','1']
    viewer=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,start_new_session=True)
    display=Display(viewer.stdin);os.set_blocking(viewer.stdout.fileno(),False)
    cancel=threading.Event();decided=threading.Event();done=threading.Event();result={}
    work_args=SimpleNamespace(**vars(args));work_args.cancel_event=cancel
    def work():
        try:
            # Niceness is per Linux task: lower this worker thread and its
            # descendants, retaining the viewer/controller's normal priority.
            os.setpriority(os.PRIO_PROCESS,threading.get_native_id(),10)
            if cancel.is_set():
                result['state']='cancelled';return
            if m.status(work_args)['state']=='ready':
                display(0,'idle');result['state']='ready';return
            notify({'state':'preparing'});display(0,'planning')
            if not decided.wait(3):display(0,'exclusive')
            if cancel.is_set():
                result['state']='cancelled';display(0,'cancelled');return
            result.update(m.prepare(work_args,display))
        except (OSError,ValueError,KeyError,TypeError,subprocess.SubprocessError):
            result['state']='cancelled' if cancel.is_set() or m.STOP or not m.ac_power() else 'unavailable'
            display(0,result['state'])
            if result['state']=='unavailable':
                try:record_failure(failure)
                except (OSError,ValueError):pass
        finally:
            if result.get('state'):notify({'state':result['state']})
            done.set()
    job=threading.Thread(target=work,daemon=True);selector=selectors.DefaultSelector();job_started=False
    selector.register(viewer.stdout,selectors.EVENT_READ,'viewer')
    try:
        try:selector.register(sys.stdin,selectors.EVENT_READ,'control')
        except (OSError,ValueError):pass
        output=bytearray();control=bytearray();total_output=0;began=time.monotonic()
        notify({'state':'checking'});job.start();job_started=True
        while viewer.poll() is None and not m.STOP:
            if not done.is_set() and time.monotonic()-began>1200:
                cancel.set();display(0,'cancelled')
            for key,_ in selector.select(.1):
                data=os.read(key.fd,4096)
                if not data:
                    selector.unregister(key.fileobj)
                    if key.data=='control' and args.managed:return 0
                    continue
                if key.data=='viewer':
                    total_output+=len(data);output.extend(data)
                    if total_output>8192 or len(output)>1024:raise ValueError('preparation viewer output exceeded its bound')
                    while b'\n' in output:
                        line,_,output=output.partition(b'\n')
                        try:
                            value=json.loads(line).get('screensafePreparation',{}).get('mode')
                            if value in ['overlay','replacement']:
                                decided.set();notify({'mode':value})
                        except (ValueError,AttributeError):pass
                else:
                    control.extend(data)
                    if len(control)>8192:raise ValueError('preparation controls exceeded their bound')
                    while b'\n' in control:
                        line,_,control=control.partition(b'\n')
                        if line==b'quit':return 0
                        if line==b'cancel-preparation':
                            cancel.set();decided.set();display(0,'cancelled');continue
                        if line.startswith(b'overlay ') and line[8:].decode('ascii')!=args.overlay:
                            # A changed live composition invalidates this build;
                            # retain the ordinary viewer's overlay handling.
                            cancel.set();display(0,'cancelled')
                        if len(line)<=1024:
                            try:os.write(viewer.stdin.fileno(),line+b'\n')
                            except (OSError,BlockingIOError):pass
        return 143 if m.STOP else viewer.returncode
    finally:
        cancel.set();decided.set();selector.close()
        try:
            if viewer.poll() is None:
                try:os.write(viewer.stdin.fileno(),b'quit\n');viewer.wait(timeout=1)
                except (OSError,subprocess.TimeoutExpired):m.terminate(viewer)
        finally:
            viewer.stdin.close();viewer.stdout.close()
            if job_started:job.join(timeout=3)
