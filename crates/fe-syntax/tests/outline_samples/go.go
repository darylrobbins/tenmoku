package main

import "fmt"

const Limit = 10

const Width, Height = 3, 4

const (
	Red = iota
	Green
)

type Point struct {
	X, Y int
}

type Shape interface {
	Area() float64
	Name() string
}

type Celsius float64

type Alias = Point

func (p *Point) Area() float64 {
	return 0
}

func (p Point) Name() string {
	return "point"
}

func main() {
	const local = 1
	fmt.Println(Point{}.Name(), local)
}
