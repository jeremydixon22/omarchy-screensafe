"""Bounded advisory stage progress. Codec messages never authorize playback."""
import json
import os
import threading
import time

class Reader:
    def __init__(self, stream, callback, *, total, stage, start, end, ffmpeg=False, log=None):
        self.stream=stream;self.callback=callback;self.total=total;self.stage=stage
        self.start=start;self.end=end;self.ffmpeg=ffmpeg;self.log=log;self.error=None
        self.thread=threading.Thread(target=self.read,daemon=True);self.thread.start()
    def read(self):
        try:
            count=0;previous=0
            while line:=self.stream.readline(513):
                count+=len(line)
                if count>1<<20 or len(line)>512 or not line.endswith(b'\n'):
                    raise ValueError('preparation progress exceeded its bounds')
                if self.log:self.log.write(line)
                if self.ffmpeg:
                    if not line.startswith(b'frame='):continue
                    text=line[6:].strip()
                    if not text.isdigit():raise ValueError('invalid encoder progress')
                    value=int(text)
                else:
                    data=json.loads(line)
                    if not isinstance(data,dict):raise ValueError('invalid verification progress object')
                    value=data.get('verifiedFrames');total=data.get('totalFrames')
                    if total!=self.total or type(value) is not int:raise ValueError('invalid verification progress')
                if not previous<=value<=self.total:raise ValueError('invalid preparation frame count')
                previous=value
                if self.callback:self.callback(self.start+(self.end-self.start)*value/self.total,self.stage)
        except (OSError,ValueError,TypeError,KeyError) as error:self.error=error
        finally:self.stream.close()
    def finish(self):
        self.thread.join(timeout=2)
        if self.thread.is_alive():raise ValueError('preparation progress did not close')
        if self.error:raise ValueError('invalid preparation progress') from self.error

class Display:
    """Small atomic nonblocking writes; a stuck viewer cannot block preparation."""
    def __init__(self, stream):
        self.fd=stream.fileno();os.set_blocking(self.fd,False)
        self.value=0;self.last=0;self.lock=threading.Lock()
    def __call__(self,value,phase):
        if phase not in ['planning','recording','software','hardware','finishing','ready','cancelled','unavailable','idle','exclusive']:return
        if not isinstance(value,(int,float)) or not 0<=value<=1:return
        with self.lock:
            self.value=max(self.value,min(1000 if phase=='ready' else 999,int(value*1000)))
            now=time.monotonic()
            if now-self.last<.2 and phase not in ['ready','cancelled','unavailable','idle','exclusive']:return
            line=f'preparation {self.value} {phase}\n'.encode()
            try:os.write(self.fd,line)
            except (BlockingIOError,BrokenPipeError,OSError):return
            self.last=now
