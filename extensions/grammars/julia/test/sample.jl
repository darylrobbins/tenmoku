#!/usr/bin/env julia
# A small geometry module, for the highlighting sample.

#=
Block comments may span lines,
#= and nest =#.
=#

module Geometry

using LinearAlgebra: norm, dot
import Base: show, +

export Point, Shape, Circle, area, describe

const ORIGIN_NAME = "origin"
const TOLERANCE = 1.0e-9

abstract type Shape end

"""
    Point{T<:Real}

A point in the plane, with `x` and `y` coordinates.
"""
struct Point{T<:Real}
    x::T
    y::T
end

mutable struct Circle <: Shape
    center::Point{Float64}
    radius::Float64
end

Base.:+(a::Point, b::Point) = Point(a.x + b.x, a.y + b.y)

show(io::IO, p::Point) = print(io, "Point($(p.x), $(p.y))")

"""
    area(c::Circle)

The area of a circle.
"""
function area(c::Circle)::Float64
    return π * c.radius^2
end

function describe(shapes::Vector{<:Shape}; verbose::Bool = false)
    total = 0.0
    for (i, s) in enumerate(shapes)
        a = area(s)
        if a < TOLERANCE
            continue
        elseif verbose
            println("shape $i:\tarea = $(round(a; digits = 2))\n")
        else
            total += a
        end
    end
    total > 0 ? total : nothing
end

macro twice(ex)
    quote
        $(esc(ex))
        $(esc(ex))
    end
end

function parse_count(text::AbstractString)
    try
        n = parse(Int, text)
        n >= 0 || throw(ArgumentError("negative: $n"))
        n
    catch err
        err isa ArgumentError ? -1 : rethrow()
    finally
        @debug "parsed" text
    end
end

let hex = 0xff, bin = 0b1010, big = 1_000_000, ch = 'λ'
    squares = [k^2 for k in 1:10 if isodd(k)]
    pairs = Dict(:a => 1, :b => 2)
    total = sum(x -> x * 2, squares) |> abs
    @twice println(hex + bin + big, ch, pairs[:a], total)
    files = `ls -la /tmp`
    note = md"**Bold** and _italics_"
    while total > 100
        total ÷= 2
    end
    local flag = true && !false
    println(raw"C:\path", r"^\d+$", flag, files, note)
end

end # module
