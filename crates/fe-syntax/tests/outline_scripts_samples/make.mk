# Builds the tool.
CC := cc
CFLAGS ?= -O2 -Wall
OBJS = main.o util.o
override LDFLAGS += -lm

ifeq ($(OS),Windows_NT)
EXE = .exe
else
EXE =
endif

.PHONY: all clean
.SUFFIXES:

all: tool

tool: CFLAGS += -g
tool: $(OBJS)
	$(CC) $(CFLAGS) -o $@ $^ $(LDFLAGS)

%.o: %.c
	$(CC) $(CFLAGS) -c $<

install uninstall: tool
	./scripts/$@.sh

libutil.a(util.o): util.o
	$(AR) cr $@ $<

$(OUT)/stamp: tool
	touch $@

help lib.a(help.o):
	@echo help

define compile-rule
$(1).o: $(1).c
	$$(CC) -c $$<
endef

clean:
	rm -f tool $(OBJS)
