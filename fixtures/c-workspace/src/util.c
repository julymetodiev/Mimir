#include "util.h"
static int local_one(int a) { return a; }
int shared_add(int a, int b) { return local_one(a) + b; }
int twice(int a) { return shared_add(a, a); }
MAKE_FN(generated)
#ifdef FEATURE
int feature_only(void) { return 1; }
#else
int feature_only(void) { return 0; }
#endif
