#define _POSIX_C_SOURCE 200809L

#include "lox_runtime.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

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
  case TAG_FUNCTION: {
    LoxClosure *closure = (LoxClosure *)(intptr_t)value.payload;
    printf("<fn %s>\n", closure->name ? closure->name : "?");
    break;
  }
  case TAG_CLASS: {
    LoxClassDesc *klass = (LoxClassDesc *)(intptr_t)value.payload;
    printf("%s\n", klass->name);
    break;
  }
  case TAG_INSTANCE: {
    LoxInstance *inst = (LoxInstance *)(intptr_t)value.payload;
    printf("%s instance\n", inst->klass->name);
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

LoxClosure *lox_alloc_closure(void *fn_ptr, int32_t arity, const char *name,
                               LoxValue **env, int32_t env_count) {
  LoxClosure *closure = malloc(sizeof(LoxClosure));
  closure->fn_ptr = fn_ptr;
  closure->arity = arity;
  closure->name = name;
  closure->env_count = env_count;
  if (env_count > 0 && env != NULL) {
    closure->env = malloc(sizeof(LoxValue *) * (size_t)env_count);
    memcpy(closure->env, env, sizeof(LoxValue *) * (size_t)env_count);
  } else {
    closure->env = NULL;
  }
  return closure;
}

LoxCell *lox_alloc_cell(LoxValue initial) {
  LoxCell *cell = malloc(sizeof(LoxCell));
  *cell = initial;
  return cell;
}

LoxValue lox_cell_get(LoxCell *cell) { return *cell; }

void lox_cell_set(LoxCell *cell, LoxValue value) { *cell = value; }

LoxValue lox_string_concat(LoxValue a, LoxValue b) {
  const char *sa = (const char *)(intptr_t)a.payload;
  const char *sb = (const char *)(intptr_t)b.payload;
  size_t la = strlen(sa);
  size_t lb = strlen(sb);
  char *result = malloc(la + lb + 1);
  memcpy(result, sa, la);
  memcpy(result + la, sb, lb + 1);
  LoxValue v;
  v.tag = TAG_STRING;
  v.payload = (int64_t)(intptr_t)result;
  return v;
}

int8_t lox_string_equal(LoxValue a, LoxValue b) {
  const char *sa = (const char *)(intptr_t)a.payload;
  const char *sb = (const char *)(intptr_t)b.payload;
  return strcmp(sa, sb) == 0 ? 1 : 0;
}

LoxClassDesc *lox_alloc_class(const char *name, LoxClassDesc *superclass,
                               int32_t method_count) {
  LoxClassDesc *klass = malloc(sizeof(LoxClassDesc));
  klass->name = name;
  klass->superclass = superclass;
  klass->method_count = 0;
  klass->methods = malloc(sizeof(LoxMethodEntry) * (size_t)method_count);
  return klass;
}

void lox_class_add_method(LoxClassDesc *klass, const char *name,
                           LoxClosure *closure) {
  klass->methods[klass->method_count].name = name;
  klass->methods[klass->method_count].closure = closure;
  klass->method_count++;
}

LoxValue lox_alloc_instance(LoxClassDesc *klass) {
  LoxInstance *inst = malloc(sizeof(LoxInstance));
  inst->klass = klass;
  inst->field_count = 0;
  LoxValue v;
  v.tag = TAG_INSTANCE;
  v.payload = (int64_t)(intptr_t)inst;
  return v;
}

static LoxInstance *extract_instance(LoxValue value) {
  return (LoxInstance *)(intptr_t)value.payload;
}

LoxClosure *lox_class_find_method(LoxClassDesc *klass, const char *name) {
  for (LoxClassDesc *k = klass; k != NULL; k = k->superclass) {
    for (int i = 0; i < k->method_count; i++) {
      if (strcmp(k->methods[i].name, name) == 0) {
        return k->methods[i].closure;
      }
    }
  }
  return NULL;
}

LoxValue lox_bind_method(LoxValue instance, LoxClosure *method) {
  /* Create a new closure with env[0] = cell containing the instance. */
  int new_env_count = method->env_count;
  LoxClosure *bound = malloc(sizeof(LoxClosure));
  bound->fn_ptr = method->fn_ptr;
  bound->arity = method->arity;
  bound->name = method->name;
  bound->env_count = new_env_count;
  bound->env = malloc(sizeof(LoxValue *) * (size_t)new_env_count);
  if (method->env != NULL) {
    memcpy(bound->env, method->env, sizeof(LoxValue *) * (size_t)new_env_count);
  }
  /* Replace env[0] with a new cell holding the instance. */
  bound->env[0] = lox_alloc_cell(instance);
  LoxValue v;
  v.tag = TAG_FUNCTION;
  v.payload = (int64_t)(intptr_t)bound;
  return v;
}

LoxValue lox_instance_get_property(LoxValue instance, const char *name,
                                    int64_t name_len) {
  LoxInstance *inst = extract_instance(instance);
  /* Check fields first. */
  for (int i = 0; i < inst->field_count; i++) {
    if ((int64_t)strlen(inst->fields[i].name) == name_len &&
        memcmp(inst->fields[i].name, name, (size_t)name_len) == 0) {
      return inst->fields[i].value;
    }
  }
  /* Then check methods (with bind). */
  /* We need a null-terminated copy for lox_class_find_method. */
  char name_buf[128];
  if (name_len >= (int64_t)sizeof(name_buf)) name_len = (int64_t)sizeof(name_buf) - 1;
  memcpy(name_buf, name, (size_t)name_len);
  name_buf[name_len] = '\0';
  LoxClosure *method = lox_class_find_method(inst->klass, name_buf);
  if (method != NULL) {
    return lox_bind_method(instance, method);
  }
  fprintf(stderr, "Error: undefined property '%.*s'\n", (int)name_len, name);
  exit(70);
}

void lox_instance_set_field(LoxValue instance, const char *name,
                             int64_t name_len, LoxValue value) {
  LoxInstance *inst = extract_instance(instance);
  /* Update existing field if present. */
  for (int i = 0; i < inst->field_count; i++) {
    if ((int64_t)strlen(inst->fields[i].name) == name_len &&
        memcmp(inst->fields[i].name, name, (size_t)name_len) == 0) {
      inst->fields[i].value = value;
      return;
    }
  }
  /* Add new field. */
  if (inst->field_count >= MAX_FIELDS) {
    fprintf(stderr, "Error: too many fields on instance\n");
    exit(70);
  }
  if (name_len >= (int64_t)sizeof(inst->fields[0].name)) {
    name_len = (int64_t)sizeof(inst->fields[0].name) - 1;
  }
  memcpy(inst->fields[inst->field_count].name, name, (size_t)name_len);
  inst->fields[inst->field_count].name[name_len] = '\0';
  inst->fields[inst->field_count].value = value;
  inst->field_count++;
}

LoxValue lox_clock(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  double secs = (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
  LoxValue v;
  v.tag = TAG_NUMBER;
  memcpy(&v.payload, &secs, sizeof(double));
  return v;
}

LoxValue lox_read_line(void) {
  char *buf = NULL;
  size_t len = 0;
  ssize_t nread = getline(&buf, &len, stdin);
  if (nread <= 0) {
    free(buf);
    LoxValue nil = {TAG_NIL, 0};
    return nil;
  }
  /* Strip trailing \r\n or \n */
  while (nread > 0 && (buf[nread - 1] == '\n' || buf[nread - 1] == '\r')) {
    buf[--nread] = '\0';
  }
  LoxValue v;
  v.tag = TAG_STRING;
  v.payload = (int64_t)(intptr_t)buf;
  return v;
}

/* Returns 1 and sets *out if s is a valid Lox NUMBER literal (after trimming).
 * Accepts DIGIT+ ("." DIGIT+)? — no sign, no scientific notation. */
static int parse_lox_number_str(const char *s, double *out) {
  /* skip leading whitespace */
  while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n') s++;
  if (*s == '\0') return 0;
  /* must start with a digit */
  const char *p = s;
  if (!(*p >= '0' && *p <= '9')) return 0;
  while (*p >= '0' && *p <= '9') p++;
  if (*p == '.') {
    p++;
    const char *after_dot = p;
    while (*p >= '0' && *p <= '9') p++;
    if (p == after_dot) return 0; /* "3." — no digits after dot */
  }
  /* skip trailing whitespace */
  while (*p == ' ' || *p == '\t' || *p == '\r' || *p == '\n') p++;
  if (*p != '\0') return 0; /* extra characters */
  *out = strtod(s, NULL);
  return 1;
}

LoxValue lox_to_number(LoxValue value) {
  LoxValue nil_val = {TAG_NIL, 0};
  if (value.tag == TAG_NUMBER) return value;
  if (value.tag != TAG_STRING) return nil_val;
  const char *s = (const char *)(intptr_t)value.payload;
  double result;
  if (!parse_lox_number_str(s, &result)) return nil_val;
  LoxValue v;
  v.tag = TAG_NUMBER;
  memcpy(&v.payload, &result, sizeof(double));
  return v;
}
