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
 *   4 = function/closure (future)
 *   5 = class (future)
 *   6 = instance (future)
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

void lox_print(LoxValue value);
LoxValue lox_global_get(const char *name, int64_t name_len);
void lox_global_set(const char *name, int64_t name_len, LoxValue value);
int8_t lox_value_truthy(LoxValue value);
void lox_runtime_error(const char *message, int64_t message_len, int32_t line);

#endif /* LOX_RUNTIME_H */
