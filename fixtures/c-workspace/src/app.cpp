#include <vector>
#include "shape.hpp"
using namespace geo;
static int helper(int a) { return a; }
int main() {
    Circle c(1.0);
    Square q;
    q.s = 1;
    Shape& s = c;
    double d = s.area();
    d += c.area();
    /* щ */ d += measure(q);
    d += pick(1);
    d += pick(1.0);
    d += biggest(1, 2);
    Box<int> b;
    b.put(1);
    d += b.get();
    Box<char> bc;
    bc.put('a');
    Vec v{1};
    Vec w = v + v;
    auto l = [](int a) { return helper(a); };
    d += l(w.x);
    int (*fp)(int) = helper;
    d += fp(1);
    std::vector<int> xs;
    xs.push_back(1);
    Two(1);
    Two t("a");
    delete new Two(1, 2);
    Made<int>(1);
    Made<char> m("a");
    int one = 1;
    Solo solo(one);
    return static_cast<int>(d);
}
