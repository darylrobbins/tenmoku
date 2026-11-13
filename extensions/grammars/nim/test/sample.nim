## A small inventory module, to show off Nim's syntax.

import std/[algorithm, strutils, tables]
from std/math import sqrt

#[ A block comment,
   #[ which nests ]#
   over several lines. ]#

##[ A doc block comment
    for the module. ]##

const
  MaxItems* = 100
  Ratio = 0.75
  Mask = 0xFF'u8
  Greeting = "Hello,\tworld!\n"
  RawPath = r"C:\temp\new"
  Banner = """
Inventory
  report"""

type
  Kind* = enum
    kTool, kFood, kPart

  Item* = object
    name*: string
    count: int
    case kind: Kind
    of kFood:
      calories: float
    else:
      discard

  Inventory* = ref object of RootObj
    items: seq[Item]
    index: Table[string, int]

  Shape = tuple[w, h: float]

proc newInventory*(): Inventory =
  ## Makes an empty inventory.
  result = Inventory(items: @[], index: initTable[string, int]())

proc add*(inv: Inventory, item: Item) {.raises: [ValueError].} =
  if inv.items.len >= MaxItems:
    raise newException(ValueError, "inventory is full")
  inv.index[item.name] = inv.items.len
  inv.items.add item

func area(s: Shape): float {.inline.} = s.w * s.h

proc total[T: SomeNumber](xs: openArray[T]): T =
  for x in xs:
    result += x

iterator named(inv: Inventory): string =
  for item in inv.items:
    yield item.name

template withLog(msg: string, body: untyped) =
  echo "start: ", msg
  body
  echo "done"

template quietly(body: untyped) = body

macro debugName(x: typed): untyped =
  newLit($x)

proc describe(item: Item): string =
  case item.kind
  of kTool: result = "tool " & item.name
  of kFood: result = "food " & item.name & " (" & $item.calories & " kcal)"
  else: result = "part"

proc grade(score: int): char =
  case score
    of 90..100: result = 'A'
    elif score >= 50: result = 'C'
    else: result = 'F'

when isMainModule:
  var inv = newInventory()
  let apple = Item(name: "apple", count: 3, kind: kFood, calories: 52.0)
  let toolKind = parseEnum[Kind]("kTool")
  inv.add apple
  let counts = inv.items.to[seq[int]]()
  let seen = initTable[string, int]()
  let blanks = newSeq[int] 3
  withLog("report"):
    for name in inv.named:
      echo name.toUpperAscii()
  withLog "audit":
    discard grade 75
  quietly:
    inv.items.sort do (a, b: Item) -> int: cmp(a.count, b.count)
  let shape: Shape = (w: 2.5, h: 4.0)
  echo area(shape), " ", total([1, 2, 3]), " ", sqrt(16.0)
  echo describe(apple), ' ', 'x', Greeting, RawPath, Banner
  var i = 0
  while i < 3 and not false:
    inc i
    if i mod 2 == 0:
      continue
  try:
    inv.add Item(name: "hammer", count: 1, kind: toolKind)
  except ValueError as e:
    echo "error: ", e.msg
  finally:
    discard Mask
  echo debugName(Ratio), nil == nil, 1_000, 3.14e-2
