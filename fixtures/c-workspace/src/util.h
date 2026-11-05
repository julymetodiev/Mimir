#ifndef UTIL_H
#define UTIL_H
typedef int (*op_fn)(int);
struct ops { op_fn run; int count; };
int shared_add(int a, int b);
int twice(int a);
static inline int in_header(int a) { return a + 1; }
#define CALL_TWICE(x) twice(x)
#define MAKE_FN(name) int name(void) { return 1; }
#endif
