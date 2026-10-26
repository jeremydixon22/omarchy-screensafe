/* MIT: trusted resource setup before Bubblewrap. Never parses media or packs.
 * Kept separate from the GPU worker so import needs no graphics/MPV libraries.
 */
#define _GNU_SOURCE
#include <sys/resource.h>
#include <sys/prctl.h>
#include <unistd.h>
#include <signal.h>
#include <stdio.h>

int main(int argc,char **argv) {
    pid_t parent=getppid();
    struct rlimit address={2ull<<30,2ull<<30},cpu={180,180},files={64,64},core={0,0},size={128u<<20,128u<<20};
    if(parent==1 || argc<2 || argv[1][0]!='/' ||
       prctl(PR_SET_PDEATHSIG,SIGTERM,0,0,0) || getppid()!=parent ||
       setrlimit(RLIMIT_AS,&address) || setrlimit(RLIMIT_CPU,&cpu) ||
       setrlimit(RLIMIT_NOFILE,&files) || setrlimit(RLIMIT_CORE,&core) || setrlimit(RLIMIT_FSIZE,&size)) {
        fputs("screensafe: cannot establish media resource limits\n",stderr);return 2;
    }
    execv(argv[1],argv+1);
    fputs("screensafe: cannot start the media sandbox\n",stderr);return 2;
}
