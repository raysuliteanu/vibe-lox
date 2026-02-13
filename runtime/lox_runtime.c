#include "lox_runtime.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Simple global variable store using a linear search table.
 * Adequate for the small number of globals in typical Lox programs.
 */

#define MAX_GLOBALS 256

typedef struct {
  char name[128];
  int64_t name_len;
  LoxValue value;
  int occupied;
} GlobalEntry;

static GlobalEntry globals[MAX_GLOBALS];
static int global_count = 0;

static GlobalEntry *find_global(const char *name, int64_t name_len) {
  for (int i = 0; i < global_count; i++) {
    if (globals[i].occupied && globals[i].name_len == name_len &&
        memcmp(globals[i].name, name, (size_t)name_len) == 0) {
      return &globals[i];
    }
  }
  return NULL;
}

static double payload_to_double(int64_t payload) {
  double d;
  memcpy(&d, &payload, sizeof(d));
  return d;
}

void lox_print(LoxValue value) {
  switch (value.tag) {
  case TAG_NIL:
    printf("nil\n");
    break;
  case TAG_BOOL:
    printf("%s\n", value.payload ? "true" : "false");
    break;
  case TAG_NUMBER: {
    double d = payload_to_double(value.payload);
    /* Print integers without trailing .0, matching Lox semantics */
    if (d == floor(d) && !isinf(d) && fabs(d) < 1e15) {
      printf("%.0f\n", d);
    } else {
      printf("%g\n", d);
    }
    break;
  }
  case TAG_STRING: {
    const char *s = (const char *)(intptr_t)value.payload;
    printf("%s\n", s);
    break;
  }
  default:
    printf("<unknown value tag %d>\n", value.tag);
    break;
  }
}

LoxValue lox_global_get(const char *name, int64_t name_len) {
  GlobalEntry *entry = find_global(name, name_len);
  if (entry) {
    return entry->value;
  }
  fprintf(stderr, "Error: undefined variable '%.*s'\n", (int)name_len, name);
  exit(70);
}

void lox_global_set(const char *name, int64_t name_len, LoxValue value) {
  GlobalEntry *entry = find_global(name, name_len);
  if (entry) {
    entry->value = value;
    return;
  }
  if (global_count >= MAX_GLOBALS) {
    fprintf(stderr, "Error: too many global variables\n");
    exit(70);
  }
  GlobalEntry *new_entry = &globals[global_count++];
  if (name_len >= (int64_t)sizeof(new_entry->name)) {
    name_len = (int64_t)sizeof(new_entry->name) - 1;
  }
  memcpy(new_entry->name, name, (size_t)name_len);
  new_entry->name[name_len] = '\0';
  new_entry->name_len = name_len;
  new_entry->value = value;
  new_entry->occupied = 1;
}

int8_t lox_value_truthy(LoxValue value) {
  switch (value.tag) {
  case TAG_NIL:
    return 0;
  case TAG_BOOL:
    return value.payload != 0;
  default:
    return 1;
  }
}

void lox_runtime_error(const char *message, int64_t message_len, int32_t line) {
  if (line > 0) {
    fprintf(stderr, "Error: line %d: %.*s\n", line, (int)message_len, message);
  } else {
    fprintf(stderr, "Error: %.*s\n", (int)message_len, message);
  }
  exit(70);
}
