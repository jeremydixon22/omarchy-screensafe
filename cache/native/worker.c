/* Waylandless prepared-video GPU worker. No pack/QML/parser authority.
 * The broker supplies one normalized, immutable /input.mp4 and one render node.
 * Only the Rust presenter owns the compositor connection. Fixed-size messages
 * and a three-buffer pool bound the IPC; the presenter controls frame cadence.
 */
#define _GNU_SOURCE
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES3/gl3.h>
#include <GLES2/gl2ext.h>
#include <gbm.h>
#include <mpv/client.h>
#include <mpv/render_gl.h>
#include <sys/socket.h>
#include <sys/eventfd.h>
#include <sys/resource.h>
#include <poll.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <time.h>
#include <sched.h>
#include <seccomp.h>
#include <sys/syscall.h>

#define MAGIC 0x31435053u /* SPC1, little endian; Linux targets are LE. */
#define XR24 0x34325258u
#define MAX_SIDE 4096u
#define MAX_PIXELS (4096u*2160u)
static mpv_handle *player;
static mpv_render_context *render;
static int wake_fd;
static unsigned width, height, frames, fps;
static const char *decoder;
struct buffer { struct gbm_bo *bo; EGLImage image; GLuint texture, fbo; int fd; uint32_t stride; };
static struct buffer buffers[3];
static void fail(const char *why) { fprintf(stderr,"screensafe-cache: %s\n",why); exit(2); }
static double seconds(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec+t.tv_nsec/1e9; }
static unsigned number(const char *s, unsigned max) {
    char *end; errno=0; unsigned long n=strtoul(s,&end,10);
    if(errno || !*s || *end || n<1 || n>max) fail("invalid bounded integer"); return (unsigned)n;
}
static void *get_proc(void *unused,const char *name) { (void)unused; return (void *)eglGetProcAddress(name); }
static void wake(void *unused) { (void)unused; uint64_t one=1; ssize_t n=write(wake_fd,&one,sizeof one); (void)n; }
static void option(const char *name,const char *value) {
    if(mpv_set_option_string(player,name,value)<0) fail("unsupported fixed player option");
}
static void write_all(int fd,const void *p,size_t n) {
    while(n) { ssize_t v=write(fd,p,n); if(v<0 && errno==EINTR)continue; if(v<=0)fail("output closed"); p=(const char *)p+v;n-=(size_t)v; }
}
static void packet(const uint32_t *words,size_t count,int fd) {
    struct iovec iov={(void *)words,count*4}; char ancillary[CMSG_SPACE(sizeof(int))]={0};
    struct msghdr msg={.msg_iov=&iov,.msg_iovlen=1};
    if(fd>=0) { msg.msg_control=ancillary;msg.msg_controllen=sizeof ancillary;
        struct cmsghdr *c=CMSG_FIRSTHDR(&msg);c->cmsg_level=SOL_SOCKET;c->cmsg_type=SCM_RIGHTS;c->cmsg_len=CMSG_LEN(sizeof(int));memcpy(CMSG_DATA(c),&fd,sizeof fd); }
    ssize_t n;do { n=sendmsg(0,&msg,MSG_NOSIGNAL); }while(n<0 && errno==EINTR);
    if(n!=(ssize_t)(count*4))fail("packet output closed");
}
static void init_gpu(void) {
    int drm=open("/dev/dri/renderD128",O_RDWR|O_CLOEXEC);if(drm<0)fail("render node unavailable");
    struct gbm_device *device=gbm_create_device(drm);if(!device)fail("GBM unavailable");
    EGLDisplay display=eglGetPlatformDisplay(EGL_PLATFORM_GBM_KHR,device,NULL);
    EGLint major,minor;if(display==EGL_NO_DISPLAY || !eglInitialize(display,&major,&minor))fail("EGL unavailable");
    if(!eglBindAPI(EGL_OPENGL_API))fail("GLES unavailable");
    EGLint attributes[]={EGL_SURFACE_TYPE,EGL_WINDOW_BIT,EGL_RENDERABLE_TYPE,EGL_OPENGL_BIT,EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_NONE};
    EGLConfig config;EGLint count;if(!eglChooseConfig(display,attributes,&config,1,&count)||count!=1)fail("no EGL configuration");
    EGLint context_attr[]={EGL_CONTEXT_MAJOR_VERSION,3,EGL_CONTEXT_MINOR_VERSION,3,EGL_NONE};
    EGLContext context=eglCreateContext(display,config,EGL_NO_CONTEXT,context_attr);
    if(context==EGL_NO_CONTEXT || !eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,context))fail("no surfaceless GLES context");
    PFNGLEGLIMAGETARGETTEXTURE2DOESPROC attach=(void *)eglGetProcAddress("glEGLImageTargetTexture2DOES");
    if(!attach)fail("no EGL image support");
    for(unsigned i=0;i<3;i++) {
        struct buffer *b=&buffers[i];
        b->bo=gbm_bo_create(device,width,height,GBM_FORMAT_XRGB8888,GBM_BO_USE_RENDERING|GBM_BO_USE_LINEAR);
        if(!b->bo || gbm_bo_get_plane_count(b->bo)!=1 || gbm_bo_get_modifier(b->bo)!=0)fail("linear GPU output unavailable");
        b->fd=gbm_bo_get_fd(b->bo);b->stride=gbm_bo_get_stride(b->bo);
        if(b->fd<0 || b->stride<width*4 || b->stride>65536)fail("invalid GPU buffer layout");
        EGLAttrib attrs[]={EGL_WIDTH,width,EGL_HEIGHT,height,EGL_LINUX_DRM_FOURCC_EXT,XR24,
            EGL_DMA_BUF_PLANE0_FD_EXT,b->fd,EGL_DMA_BUF_PLANE0_OFFSET_EXT,0,EGL_DMA_BUF_PLANE0_PITCH_EXT,b->stride,EGL_NONE};
        b->image=eglCreateImage(display,EGL_NO_CONTEXT,EGL_LINUX_DMA_BUF_EXT,NULL,attrs);
        if(b->image==EGL_NO_IMAGE)fail("GPU buffer import failed");
        glGenTextures(1,&b->texture);glBindTexture(GL_TEXTURE_2D,b->texture);attach(GL_TEXTURE_2D,b->image);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);
        glGenFramebuffers(1,&b->fbo);glBindFramebuffer(GL_FRAMEBUFFER,b->fbo);
        glFramebufferTexture2D(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_TEXTURE_2D,b->texture,0);
        if(glCheckFramebufferStatus(GL_FRAMEBUFFER)!=GL_FRAMEBUFFER_COMPLETE)fail("GPU framebuffer unavailable");
    }
    glBindFramebuffer(GL_FRAMEBUFFER,0);glBindTexture(GL_TEXTURE_2D,0);
    player=mpv_create();if(!player)fail("player allocation failed");
    const char *opts[][2]={{"vo","libmpv"},{"hwdec","no"},{"vaapi-device","/dev/dri/renderD128"},
        {"hwdec-extra-frames","6"},{"vd-queue-enable","no"},{"vd-lavc-threads","2"},{"hwdec-codecs","h264"},{"profile","fast"},{"config","no"},{"load-scripts","no"},
        {"audio","no"},{"sid","no"},{"sub-auto","no"},{"osc","no"},{"osd-level","0"},
        {"input-default-bindings","no"},{"input-terminal","no"},{"terminal","no"},{"cache","no"},
        {"pause","yes"},{"untimed","yes"},{"keep-open","yes"},{"idle","yes"},{"framedrop","no"},
        {"scale","bilinear"},{"cscale","bilinear"},{"dscale","bilinear"},{"dither-depth","no"},
        {"deband","no"},{"interpolation","no"},{"target-prim","bt.709"},{"target-trc","srgb"},
        {"icc-profile-auto","no"},{"target-colorspace-hint","no"},{"fbo-format","rgba8"}};
    for(unsigned i=0;i<sizeof opts/sizeof opts[0];i++)option(opts[i][0],opts[i][1]);
    option("hwdec",decoder);
    if(mpv_initialize(player)<0)fail("player initialization failed");
    wake_fd=eventfd(0,EFD_CLOEXEC|EFD_NONBLOCK);if(wake_fd<0)fail("wakeup allocation failed");
    static mpv_opengl_init_params gl;gl.get_proc_address=get_proc;int advanced=1;
    static mpv_opengl_drm_params_v2 native;native.fd=-1;native.render_fd=drm;
    mpv_render_param params[]={{MPV_RENDER_PARAM_API_TYPE,MPV_RENDER_API_TYPE_OPENGL},
        {MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,&gl},{MPV_RENDER_PARAM_ADVANCED_CONTROL,&advanced},
        {MPV_RENDER_PARAM_DRM_DISPLAY_V2,&native},{0,NULL}};
    if(mpv_render_context_create(&render,player,params)<0)fail("GPU player initialization failed");
    mpv_render_context_set_update_callback(render,wake,NULL);
}
static void draw(unsigned index,unsigned slot) {
    double until=seconds()+5.0;
    for(;;) {
        while(1) { mpv_event *e=mpv_wait_event(player,0);if(e->event_id==MPV_EVENT_NONE)break;
            if(e->event_id==MPV_EVENT_SHUTDOWN || e->event_id==MPV_EVENT_END_FILE)fail("video ended early");
            if(e->event_id==MPV_EVENT_COMMAND_REPLY && e->error<0)fail("video command failed"); }
        uint64_t flags=mpv_render_context_update(render);
        if(flags&MPV_RENDER_UPDATE_FRAME) {
            mpv_render_frame_info info={0};mpv_render_param query={MPV_RENDER_PARAM_NEXT_FRAME_INFO,&info};
            if(mpv_render_context_get_info(render,query)<0)fail("frame query failed");
            if(info.flags&MPV_RENDER_FRAME_INFO_PRESENT) {
                mpv_opengl_fbo fbo={(int)buffers[slot].fbo,(int)width,(int)height,GL_RGBA8};
                int flip=1,block=0;mpv_render_param p[]={{MPV_RENDER_PARAM_OPENGL_FBO,&fbo},
                    {MPV_RENDER_PARAM_FLIP_Y,&flip},{MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,&block},{0,NULL}};
                glBindFramebuffer(GL_FRAMEBUFFER,0);
                if(mpv_render_context_render(render,p)<0)fail("GPU draw failed");
                glFinish();if(glGetError()!=GL_NO_ERROR)fail("GPU draw error");
                mpv_render_context_report_swap(render);
                if(info.flags & (MPV_RENDER_FRAME_INFO_REDRAW|MPV_RENDER_FRAME_INFO_REPEAT)) continue;
                double pos=-1;if(mpv_get_property(player,"time-pos",MPV_FORMAT_DOUBLE,&pos)<0)fail("no frame timestamp");
                double expected=(double)index/fps;
                if(pos<expected-0.002 || pos>expected+0.002)fail("unexpected frame order");
                char *hw=mpv_get_property_string(player,"hwdec-current");
                if(!hw || strcmp(hw,decoder))fail("approved decoder unavailable");mpv_free(hw);
                return;
            }
        }
        if(seconds()>until)fail("frame deadline exceeded");
        struct pollfd p={wake_fd,POLLIN,0};poll(&p,1,20);uint64_t v;while(read(wake_fd,&v,sizeof v)>0){}
    }
}
static void advance(unsigned index) {
    const char *step[]={"frame-step","1","play",NULL};
    const char *seek[]={"seek","0","absolute+exact",NULL};
    if(mpv_command_async(player,2,index?step:seek)<0)fail("frame advance failed");
}
static void restrict_execution(void) {
    /* The mount/network/PID namespaces are established by the broker. Apply
     * this second layer to all existing library threads before any frame is
     * served. Libraries may create threads, never executable child processes.
     * ENOSYS lets pthread's clone3 path use the inspectable clone flags. */
    scmp_filter_ctx filter=seccomp_init(SCMP_ACT_ALLOW);
    if(!filter || seccomp_attr_set(filter,SCMP_FLTATR_CTL_TSYNC,1)<0)fail("cannot restrict worker threads");
    const char *blocked[]={"execve","execveat","fork","vfork","unshare","setns","ptrace","process_vm_readv","process_vm_writev","io_uring_setup"};
    for(unsigned i=0;i<sizeof blocked/sizeof blocked[0];i++) {
        int nr=seccomp_syscall_resolve_name(blocked[i]);
        if(nr!=__NR_SCMP_ERROR && seccomp_rule_add(filter,SCMP_ACT_ERRNO(EPERM),nr,0)<0)fail("cannot build worker filter");
    }
    int clone3_nr=seccomp_syscall_resolve_name("clone3");
    if(clone3_nr!=__NR_SCMP_ERROR && seccomp_rule_add(filter,SCMP_ACT_ERRNO(ENOSYS),clone3_nr,0)<0)fail("cannot restrict clone3");
    if(seccomp_rule_add(filter,SCMP_ACT_ERRNO(EPERM),SCMP_SYS(clone),1,
        SCMP_A0(SCMP_CMP_MASKED_EQ,CLONE_THREAD,0))<0 || seccomp_load(filter)<0)fail("cannot restrict worker execution");
    seccomp_release(filter);
    errno=0;
    if(syscall(SYS_execve,NULL,NULL,NULL)!=-1 || errno!=EPERM)fail("worker execution restriction ineffective");
}
int main(int argc,char **argv) {
    if(argc!=7 || (strcmp(argv[1],"capture") && strcmp(argv[1],"verify") && strcmp(argv[1],"serve")))fail("usage: worker capture|verify|serve WIDTH HEIGHT FRAMES FPS copy|software");
    if(!strcmp(argv[6],"copy"))decoder="vaapi-copy";
    else if(!strcmp(argv[6],"software"))decoder="no";
    else fail("invalid decoder profile");
    width=number(argv[2],MAX_SIDE);height=number(argv[3],MAX_SIDE);frames=number(argv[4],3600);fps=number(argv[5],24);
    if((uint64_t)width*height>MAX_PIXELS)fail("canvas exceeds limit");
    struct rlimit files={96,96},core={0,0},log_size={128u<<20,128u<<20},heap={1024u<<20,1024u<<20};setrlimit(RLIMIT_NOFILE,&files);setrlimit(RLIMIT_CORE,&core);setrlimit(RLIMIT_FSIZE,&log_size);setrlimit(RLIMIT_DATA,&heap);
    init_gpu();
    restrict_execution();
    /* No media parsing starts before the all-thread execution restriction. */
    const char *load[]={"loadfile","/input.mp4",NULL};
    if(mpv_command_async(player,1,load)<0)fail("video open failed");
    if(strcmp(argv[1],"serve")) {
        size_t size=(size_t)width*height*4;unsigned char *rgba=malloc(size);if(!rgba)fail("capture allocation failed");
        unsigned total=frames*(!strcmp(argv[1],"verify")?2:1);
        for(unsigned i=0;i<total;i++) {
            unsigned index=i%frames;
            if(i)advance(index);draw(index,0);glBindFramebuffer(GL_FRAMEBUFFER,buffers[0].fbo);
            glReadPixels(0,0,width,height,GL_RGBA,GL_UNSIGNED_BYTE,rgba);if(glGetError()!=GL_NO_ERROR)fail("capture failed");
            for(size_t k=3;k<size;k+=4)rgba[k]=255;
            glBindFramebuffer(GL_FRAMEBUFFER,0);
            write_all(1,rgba,size);
        }
        free(rgba);fprintf(stderr,"screensafe-cache: captured %u opaque GPU frames\n",total);
    } else {
        for(unsigned i=0;i<3;i++){uint32_t h[]={MAGIC,1,i,width,height,buffers[i].stride,XR24,0};packet(h,8,buffers[i].fd);}
        unsigned expected=0;
        for(;;) {
            uint32_t req[4];ssize_t n=recv(0,req,sizeof req,MSG_TRUNC);
            if(n<0 && errno==EINTR)continue;if(n==0)break;
            if(n!=sizeof req || req[0]!=MAGIC || req[1]!=2 || req[2]!=expected || req[3]>=3)fail("invalid frame request");
            static int first=1;if(!first)advance(expected);first=0;
            draw(expected,req[3]);uint32_t ready[]={MAGIC,3,expected,req[3]};packet(ready,4,-1);
            expected=(expected+1)%frames;
        }
    }
    mpv_render_context_set_update_callback(render,NULL,NULL);mpv_render_context_free(render);mpv_terminate_destroy(player);
    return 0;
}
