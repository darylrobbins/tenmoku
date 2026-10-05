#include <stdio.h>

#define LIMIT 10
#define SQUARE(x) ((x) * (x))

struct point {
    int x;
    int y;
};

struct point *origin_ref;

typedef struct {
    int width;
    int height;
} size;

typedef struct node {
    struct node *next;
} node_t;

union value {
    int i;
    float f;
};

enum color {
    RED,
    GREEN,
};

typedef unsigned long ulong;

static int add(int a, int b) {
    return a + b;
}

char *name(void) {
    return "c";
}

int main(void) {
    printf("%d\n", SQUARE(LIMIT));
    return 0;
}
