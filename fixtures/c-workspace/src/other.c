#include "util.h"
static int local_one(int a) { return a + 2; }
int other(int a) { return local_one(a); }
