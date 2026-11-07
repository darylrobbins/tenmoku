//// A small inventory module.
////
//// Module docs are *Markdown*.

import gleam/int
import gleam/io
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/string as str

@external(erlang, "erlang", "system_time")
fn system_time() -> Int

pub const max_items = 100

const pi = 3.14159

const mask = 0xFF

/// The kinds of item we stock.
///
/// - `Tool` has a name
/// - `Part` has a [number](https://example.com)
pub type Item {
  Tool(name: String, weight: Float)
  Part(number: Int)
  Missing
}

pub opaque type Inventory {
  Inventory(items: List(Item), owner: Option(String))
}

pub type Pair(a, b) =
  #(a, b)

// Build an empty inventory.
pub fn new() -> Inventory {
  Inventory(items: [], owner: None)
}

/// Adds an item, unless the inventory is full.
pub fn add(inv: Inventory, item: Item) -> Result(Inventory, String) {
  case list.length(inv.items) >= max_items {
    True -> Error("inventory full\n")
    False -> Ok(Inventory(..inv, items: [item, ..inv.items]))
  }
}

pub fn describe(item: Item) -> String {
  case item {
    Tool(name, weight) if weight >. 10.0 -> "heavy " <> name
    Tool(name: name, ..) -> "tool \"" <> name <> "\""
    Part(number) -> "part #" <> int.to_string(number)
    Missing -> "missing\t(\u{1F4A5})"
  }
}

pub fn total(items: List(Item)) -> Int {
  items
  |> list.map(fn(item) {
    case item {
      Part(n) -> n * 2 + 1
      _ -> 0
    }
  })
  |> list.fold(0, fn(acc, n) { acc + n })
}

fn bits() -> BitArray {
  <<1, 2:size(8), "hi":utf8>>
}

pub fn main() {
  let assert Ok(inv) = add(new(), Part(42))
  let point = #(1, 2.5e3)
  use name <- result_or("fallback")
  let ok = !False && 1_000 != 0b101
  echo point.0
  io.println(str.uppercase(name))
  todo as "finish me"
}

fn result_or(default: String, next: fn(String) -> a) -> a {
  next(default)
}
