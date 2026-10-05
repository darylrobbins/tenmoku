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

typedef int numbers[4];

typedef void (*callback)(int);

typedef void (/* note */*noted_callback)(int);

typedef int pair_a, pair_b;

static int add(int a, int b) {
    return a + b;
}

char *name(void) {
    return "c";
}

char ***names(void) {
    return 0;
}

int (twice)(int x) {
    return 2 * x;
}

int (/* note */thrice)(int x) {
    return 3 * x;
}

void (*handler(int sig, void (*fallback)(int)))(int) {
    return fallback;
}

int main(void) {
    printf("%d\n", SQUARE(LIMIT));
    return 0;
}
