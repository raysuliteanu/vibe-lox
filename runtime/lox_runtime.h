#ifndef LOX_RUNTIME_H
#define LOX_RUNTIME_H

#include <stdint.h>

/*
 * LoxValue: tagged union matching the LLVM IR struct { i8, i64 }.
 *
 * Tag values:
 *   0 = nil
 *   1 = bool   (payload: 0 or 1)
 *   2 = number (payload: f64 bitcast to i64)
 *   3 = string (payload: pointer to null-terminated C string, cast to i64)
 *   4 = function/closure (payload: pointer to LoxClosure, cast to i64)
 *   5 = class (payload: pointer to LoxClassDesc, cast to i64)
 *   6 = instance (payload: pointer to LoxInstance, cast to i64)
 */
typedef struct {
    int8_t tag;
    int64_t payload;
} LoxValue;

#define TAG_NIL      0
#define TAG_BOOL     1
#define TAG_NUMBER   2
#define TAG_STRING   3
#define TAG_FUNCTION 4
#define TAG_CLASS    5
#define TAG_INSTANCE 6

/* Heap-allocated closure: function pointer + captured environment. */
typedef struct {
    void *fn_ptr;       /* pointer to the LLVM function */
    int32_t arity;      /* number of Lox parameters (not counting env) */
    int32_t env_count;  /* number of captured cells */
    LoxValue **env;     /* array of pointers to heap-allocated cells */
    const char *name;   /* function name for printing */
} LoxClosure;

/* A cell is a heap-allocated single LoxValue, used for captured variables
 * so mutations are shared between the closure and the enclosing scope. */
typedef LoxValue LoxCell;

void lox_print(LoxValue value);
LoxValue lox_global_get(const char *name, int64_t name_len);
void lox_global_set(const char *name, int64_t name_len, LoxValue value);
int8_t lox_value_truthy(LoxValue value);
void lox_runtime_error(const char *message, int64_t message_len, int32_t line);

/* Closure/cell allocation */
LoxClosure *lox_alloc_closure(void *fn_ptr, int32_t arity, const char *name,
                               LoxValue **env, int32_t env_count);
LoxCell *lox_alloc_cell(LoxValue initial);
LoxValue lox_cell_get(LoxCell *cell);
void lox_cell_set(LoxCell *cell, LoxValue value);

/* String operations */
LoxValue lox_string_concat(LoxValue a, LoxValue b);
int8_t lox_string_equal(LoxValue a, LoxValue b);

/* Class/instance types and operations */
typedef struct {
    const char *name;
    LoxClosure *closure;
} LoxMethodEntry;

typedef struct LoxClassDesc {
    const char *name;
    struct LoxClassDesc *superclass;
    int32_t method_count;
    LoxMethodEntry *methods;
} LoxClassDesc;

#define MAX_FIELDS 256
typedef struct {
    LoxClassDesc *klass;
    int32_t field_count;
    struct { char name[128]; LoxValue value; } fields[MAX_FIELDS];
} LoxInstance;

LoxClassDesc *lox_alloc_class(const char *name, LoxClassDesc *superclass,
                               int32_t method_count);
void lox_class_add_method(LoxClassDesc *klass, const char *name,
                           LoxClosure *closure);
LoxValue lox_alloc_instance(LoxClassDesc *klass);
LoxValue lox_instance_get_property(LoxValue instance, const char *name,
                                    int64_t name_len);
void lox_instance_set_field(LoxValue instance, const char *name,
                             int64_t name_len, LoxValue value);
LoxClosure *lox_class_find_method(LoxClassDesc *klass, const char *name);
LoxValue lox_bind_method(LoxValue instance, LoxClosure *method);

/* Native functions */
LoxValue lox_clock(void);

#endif /* LOX_RUNTIME_H */
