#ifndef ETHERSYNC_EXAMPLE_CHECK_H
#define ETHERSYNC_EXAMPLE_CHECK_H
#include <stdio.h>
#include <stdlib.h>
#define CHECK(condition) do { if (!(condition)) { \
    fprintf(stderr, "%s:%d: check failed: %s\n", __FILE__, __LINE__, #condition); \
    exit(1); \
} } while (0)
#ifdef _WIN32
#include <windows.h>
static inline void smoke_sleep(void) { Sleep(10); }
#else
#include <time.h>
static inline void smoke_sleep(void) {
    struct timespec delay = {0, 10000000};
    nanosleep(&delay, NULL);
}
#endif
#endif
