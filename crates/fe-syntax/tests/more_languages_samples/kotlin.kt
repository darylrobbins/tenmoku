package demo.shapes

const val LIMIT = 10
val origin = 0
var counter = 0

interface Shape {
    val area: Double
    fun describe(): String
}

data class Point(val x: Double, val y: Double = origin.toDouble())

open class Circle(private val radius: Double) : Shape {
    override val area: Double = radius * radius
    var label = "circle"

    constructor() : this(1.0)

    override fun describe(): String {
        val local = label
        return local
    }

    companion object Factory {
        fun unit(): Circle = Circle()
    }
}

enum class Direction {
    NORTH,
    SOUTH;

    fun opposite(): Direction = this
}

object Registry {
    val shapes = mutableListOf<Shape>()

    fun register(shape: Shape) {
        shapes.add(shape)
    }
}

typealias Distance = Double

fun helper(value: Int): Int {
    val doubled = value * 2
    return doubled
}
