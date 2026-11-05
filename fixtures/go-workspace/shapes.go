// Package fixture holds the traps of docs/GO.md: every row of its section 3. What each line must
// give is listed by hand in crates/latt-resolve-go/tests/fixture/expected.rs.
package fixture

import "example.com/fixture/inner"

const Limit = 10

var Count int

// Handler is a function bound to a variable.
var Handler = func() int { return helper() }

var table = helper

var started = helper()

type Area interface {
	Area() float64
}

type Named interface {
	Name() string
}

type Both interface {
	Named
	Size() int
}

type Circle struct {
	R float64
}

func (c Circle) Area() float64 { return c.R * c.R }

type Square struct {
	S float64
}

func (s Square) Area() float64 { return s.S * s.S }

type Tag struct {
	Circle
	Label string
}

func (t Tag) Name() string { return t.Label }

func (t Tag) Size() int { return len(t.Label) }

type Outer struct {
	inner.Base
}

type MyInt int

type Fn func() int

type Stack[T any] struct {
	items []T
}

func (s *Stack[T]) Push(x T) { s.items = append(s.items, x) }

func Map[T any](x T) T { return x }

func helper() int { return 1 }

func init() { Count = 1 }

func init() { Count = 2 }
