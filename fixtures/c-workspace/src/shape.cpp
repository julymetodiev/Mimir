#include "shape.hpp"
namespace geo {
Shape::~Shape() {}
Circle::Circle(double r) : r(r) {}
double Circle::area() const { return r * r; }
double Square::area() const { return s * s; }
double measure(const Shape& s) { return s.area(); }
int pick(int a) { return a; }
double pick(double a) { return a; }
void Box<char>::put(char x) { v = x; }
Vec Vec::operator+(const Vec& o) const { return Vec{x + o.x}; }
Two::Two(int a) {}
Two::Two(const char* a) {}
Two::Two(int a, int b) {}
}
