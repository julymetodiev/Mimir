package fixture

import (
	"fmt"

	alias "example.com/fixture/inner"
)

func direct() int { return helper() }

func qualified() int { return alias.Plain() }

func spawn() {
	go helper()
	defer helper()
	func() { helper() }()
}

func generics() {
	Map[int](1)
	st := &Stack[int]{}
	st.Push(1)
}

func onInterface(a Area) float64 { return a.Area() }

func onConcrete(c Circle, s Square) float64 { return c.Area() + s.Area() }

func throughEmbedded(t Tag) float64 { return t.Area() }

func fromOtherPackage(o Outer) string { return o.Hello() }

func typeSwitch(x any) float64 {
	switch v := x.(type) {
	case Circle:
		return v.Area()
	}
	return 0
}

func embeddedInterface(n Both) string { return n.Name() }

func bound() int {
	f := helper
	return f()
}

func methodValues(c Circle) {
	g := c.Area
	h := Circle.Area
	_, _ = g, h
}

func conversions() {
	_ = Fn(helper)
	_ = MyInt(1)
	_ = float64(1)
	_ = len("")
}

func reads() int { return Limit + Count }

func fields(c *Circle) float64 {
	c.R = 1
	return c.R
}

func literal() Circle { return Circle{R: 1} }

func callsVar() int { return Handler() }

// What the source does not spell gives no reference, and `fmt` is outside the project.
func notSpelled(c Tag) string { return fmt.Sprint(c) }
