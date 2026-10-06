package main

import "fmt"

const Limit = 10

var counter int

var x, y = 1, 2

var (
	name    = "demo"
	enabled bool
)

type Point struct {
	X, Y int
	Label string `json:"label"`
	fmt.Stringer
}

type Shape interface {
	Area() float64
}

func (p Point) Area() float64 {
	var local = 1.0
	return local
}

func main() {
	var inner = Point{}
	fmt.Println(inner, counter, x, y, name, enabled)
}
