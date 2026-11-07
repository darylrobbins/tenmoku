namespace Tenmoku.Samples

open System
open System.Collections.Generic

/// A shape, measured in metres.
type Shape =
    | Circle of radius: float
    | Rectangle of width: float * height: float
    | Point

[<Measure>]
type m

(* A block comment
   spanning lines. *)
type Account(owner: string, balance: decimal) =
    let mutable current = balance
    member this.Owner = owner
    member this.Balance = current
    member this.Deposit(amount: decimal) =
        if amount <= 0m then
            invalidArg (nameof amount) "must be positive"
        current <- current + amount
    override _.ToString() = sprintf "%s: %M" owner current

type Colour =
    | Red = 0
    | Green = 1
    | Blue = 0x2

module Geometry =
    [<Literal>]
    let MaxSides = 12

    let pi = 3.14159
    let distance = 42.5<m>

    let area shape =
        match shape with
        | Circle r -> pi * r * r
        | Rectangle (w, h) -> w * h
        | Point -> 0.0

    let rec factorial n =
        if n <= 1L then 1L else n * factorial (n - 1L)

    let describe (shape: Shape) : string =
        match shape with
        | Circle r when r > 10.0 -> "a large circle"
        | Circle _ -> "a circle"
        | _ -> "something else"

module Text =
    let greeting = "Hello,\tworld!\n"
    let path = @"C:\temp\file.txt"
    let quoted = """She said "hi"."""
    let letter = 'x'
    let newline = '\n'
    let name = "Ada"
    let message = $"Hi {name}, you are {36 + 1} years old"
    let folder = $@"C:\Users\Shared {name} {{docs}}"
    let report = $"""Name: "{name}", length {name.Length}"""
    let flag = true && not false
    let nothing : string option = None

    let shout (s: string) = s.ToUpper()

    let words (text: string) =
        text.Split(' ')
        |> Array.filter (fun w -> w.Length > 0)
        |> Array.map shout
        |> List.ofArray

module Async =
    let fetchAll (urls: string list) =
        async {
            let! results =
                urls
                |> List.map (fun url -> async { return url.Length })
                |> Async.Parallel
            return Array.sum results
        }

    let evens =
        seq {
            for i in 0 .. 2 .. 10 do
                yield i
        }

    let tryParse (s: string) =
        try
            Some(Int32.Parse s)
        with
        | :? FormatException -> None

    let safeDivide x y =
        if y = 0 then failwith "divide by zero"
        else x / y

    let lookup = Dictionary<string, int>()

    let total =
        let mutable sum = 0
        while sum < 100 do
            sum <- sum + 7
        sum

    let record = {| Name = "Ada"; Age = 36 |}
