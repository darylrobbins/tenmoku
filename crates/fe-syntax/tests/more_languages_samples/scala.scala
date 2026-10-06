package demo.shapes

trait Shape {
  def area: Double
  val name: String
}

case class Point(x: Double, y: Double)(tag: String)

class Circle(radius: Double) extends Shape {
  val name = "circle"
  var scale: Double = 1.0

  def area: Double = {
    val squared = radius * radius
    squared * scale
  }
}

object Registry {
  val shapes = List.empty[Shape]

  def register(shape: Shape): Unit = ()

  type Id = Int
}

enum Direction {
  case North, South
  case Custom(degrees: Int)

  def opposite: Direction = this
}

def helper(value: Int): Int = {
  val doubled = value * 2
  doubled
}

val limit = 10
val (low, high) = (0, 10)
given ordering: Ordering[Int] = Ordering.Int
