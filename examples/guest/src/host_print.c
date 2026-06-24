#include "host_print.h"

#include <stdio.h>

int host_print(const char *s, size_t len) {
    int n = printf("%.*s", (int)len, s);
    fflush(stdout);
    return n;
}
