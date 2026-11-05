#include <stdio.h>
#include "util.h"
int feature_only(void);
int generated(void);
int main(void) {
    struct ops o;
    o.run = twice;
    o.count = 1;
    int r = o.run(o.count);
    r += shared_add(r, 1);
    r += CALL_TWICE(r);
    r += in_header(r);
    r += feature_only();
    r += generated();
    printf("%d", r);
    return r;
}
