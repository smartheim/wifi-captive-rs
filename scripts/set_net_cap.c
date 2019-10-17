// Released to public domain - David Graeff, 2019

// Compile with "gcc -o scripts/set_net_cap scripts/set_net_cap.c && sudo chown root:root scripts/set_net_cap && sudo chmod +s scripts/set_net_cap"
#include <stdio.h>
#include <stdlib.h>
#include <sys/types.h>
#include <unistd.h>
#include <limits.h>

#define eprintf(...) fprintf (stderr, __VA_ARGS__)

int main(int argc, char** argv) {
    if(geteuid() != 0)
    {
        eprintf("Must be root or setuid");
        return -1;
    }

    if (argc != 2) {
        eprintf("Relative file path not given!");
        return -1;
    }

    char cwd[PATH_MAX];
    if (getcwd(cwd, sizeof(cwd)) == NULL) {
        eprintf("getcwd() error");
        return 1;
    }

    char buffer[1000];
    snprintf(buffer, 1000, "%s/%s", cwd, argv[1]);

    if( access( buffer, F_OK ) == -1 ) {
        eprintf("file %s doesn't exist", buffer);
        return -1;
    }

    setuid(0);

    // Set capabilities
    snprintf(buffer, 1000, "setcap CAP_NET_BIND_SERVICE=+eip \"%s/%s\"", cwd, argv[1]);
    system(buffer);
}