/* A line index for a text buffer, used as a golden highlighting sample. */
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include "line_index.h"

#define INITIAL_CAPACITY 64
#define MAX(a, b) ((a) > (b) ? (a) : (b))

typedef struct {
    size_t *starts;
    size_t len;
    size_t capacity;
} LineIndex;

enum Status { STATUS_OK = 0, STATUS_NO_MEMORY = -1 };

static const char NEWLINE = '\n';

static enum Status push(LineIndex *index, size_t start) {
    if (index->len == index->capacity) {
        size_t capacity = MAX(index->capacity * 2, INITIAL_CAPACITY);
        size_t *grown = realloc(index->starts, capacity * sizeof *grown);
        if (grown == NULL) {
            return STATUS_NO_MEMORY;
        }
        index->starts = grown;
        index->capacity = capacity;
    }
    index->starts[index->len++] = start;
    return STATUS_OK;
}

enum Status line_index_build(LineIndex *index, const char *text, size_t len) {
    memset(index, 0, sizeof(*index));
    if (push(index, 0) != STATUS_OK) {
        return STATUS_NO_MEMORY;
    }
    for (size_t i = 0; i < len; i++) {
        if (text[i] == NEWLINE && push(index, i + 1) != STATUS_OK) {
            return STATUS_NO_MEMORY;
        }
    }
    return STATUS_OK;
}

/* Returns the row containing byte `offset`, by binary search. */
size_t line_index_row(const LineIndex *index, size_t offset) {
    size_t low = 0, high = index->len;
    while (high - low > 1) {
        size_t mid = low + (high - low) / 2;
        if (index->starts[mid] <= offset) {
            low = mid;
        } else {
            high = mid;
        }
    }
    return low;
}

void line_index_free(LineIndex *index) {
    free(index->starts);
    *index = (LineIndex){0};
}

#ifdef LINE_INDEX_MAIN
#include <stdio.h>
int main(int argc, char **argv) {
    const char *text = argc > 1 ? argv[1] : "one\ntwo\nthree";
    LineIndex index;
    bool ok = line_index_build(&index, text, strlen(text)) == STATUS_OK;
    printf("%s: %zu rows, 0x%x\n", ok ? "ok" : "failed", index.len, 0xffu);
    line_index_free(&index);
    return ok ? EXIT_SUCCESS : EXIT_FAILURE;
}
#endif
