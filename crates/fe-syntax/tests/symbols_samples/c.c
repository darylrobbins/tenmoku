#include <stdio.h>

#ifndef DEMO_H
#define DEMO_H
extern int shared;
#endif

struct point {
    int x, y;
    char *label;
    int (*callback)(int);
    unsigned flags : 4;
    struct {
        int depth;
    } inner;
};

union value {
    int i;
    float f;
};

enum color {
    RED,
    GREEN = 2,
};

enum { ANON_A, ANON_B };

typedef struct {
    int width;
} size;

int g;
static int counter = 0, *cursor;
const char *names[4];
int (*handler)(int);
int prototype(void);
char *named(int x);
struct point origin = {0};

static int add(int a, int b) {
    int local = a + b;
    return local;
}
