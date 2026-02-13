CC ?= cc
CFLAGS = -Wall -Wextra -O2 -fPIC

.PHONY: all clean

all: liblox_runtime.so

liblox_runtime.so: lox_runtime.c lox_runtime.h
	$(CC) $(CFLAGS) -shared -o $@ lox_runtime.c -lm

clean:
	rm -f liblox_runtime.so
