import Foundation

let limit = 10
let (low, high) = (0, 10)
var counter = 0

protocol Shape {
    var area: Double { get }
    func describe() -> String
    init(size: Double)
}

struct Point {
    var x: Double
    let y: Double

    func distance(to other: Point) -> Double {
        let dx = x - other.x
        return dx
    }
}

class Circle: Shape {
    var radius: Double
    static var count = 0

    required init(size: Double) {
        radius = size
    }

    deinit {}

    var area: Double {
        return radius * radius
    }

    func describe() -> String {
        return "circle"
    }

    subscript(index: Int) -> Double {
        return radius
    }
}

enum Direction {
    case north
    case south, east

    func opposite() -> Direction {
        return self
    }
}

extension Point {
    func scaled(by factor: Double) -> Point {
        return self
    }
}

typealias Distance = Double

@freestanding(expression)
macro stringify<T>(_ value: T) -> (T, String) = #externalMacro(module: "Macros", type: "StringifyMacro")

func helper(value: Int) -> Int {
    let doubled = value * 2
    return doubled
}
