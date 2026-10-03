# A small build file, used as a golden highlighting sample.

CC ?= cc
CFLAGS := -O2 -Wall -Wextra
LDLIBS += -lm
PREFIX = /usr/local

SRC := $(wildcard src/*.c)
OBJ := $(patsubst src/%.c,build/%.o,$(SRC))
BIN := build/app

ifeq ($(DEBUG),1)
CFLAGS += -g -O0
else
CFLAGS += -DNDEBUG
endif

ifdef VERBOSE
Q :=
else
Q := @
endif

export PATH := $(PREFIX)/bin:$(PATH)

.PHONY: all clean install test

all: $(BIN)

$(BIN): $(OBJ) | build
	$(Q)$(CC) $(CFLAGS) -o $@ $^ $(LDLIBS)

build/%.o: src/%.c
	$(Q)$(CC) $(CFLAGS) -c -o $@ $<

build:
	mkdir -p $@

install: $(BIN)
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 $(BIN) $(DESTDIR)$(PREFIX)/bin/

test: $(BIN)
	./$(BIN) --self-test
	@echo "tests passed for $(notdir $(BIN))"

define banner
	@echo "== $(1) =="
endef

check: test
	$(call banner,check)
	$(foreach f,$(SRC),echo $(f);)

clean:
	-rm -rf build

include config.mk
-include $(OBJ:.o=.d)

$(info building with $(CC))
