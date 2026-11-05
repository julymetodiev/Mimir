#pragma once
namespace geo {
struct Shape { virtual double area() const = 0; virtual ~Shape(); };
struct Circle : Shape { double r; explicit Circle(double r); double area() const override; };
struct Square : Shape { double s; double area() const override; };
double measure(const Shape& s);
int pick(int a);
double pick(double a);
template <class T> T biggest(T a, T b) { return a > b ? a : b; }
template <class T> struct Box { T v; void put(T x) { v = x; } T get() const; };
template <class T> T Box<T>::get() const { return v; }
template <> struct Box<char> { char v; void put(char x); };
struct Vec { int x; Vec operator+(const Vec& o) const; };
struct Two { Two(int a); Two(const char* a); Two(int a, int b); };
template <class T> struct Made { Made(int a) {} Made(const char* a) {} };
struct Solo { Solo(int a) {} Solo(const char* a) {} };
}
