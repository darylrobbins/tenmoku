// A short report template.
/* Block comments
   span lines. */
#import "@preview/cetz:0.2.2": canvas, draw
#set page(paper: "a4", margin: (x: 2cm, y: 2.5cm))
#set text(font: "New Computer Modern", size: 11pt, lang: "en")
#set heading(numbering: "1.1")

#let accent = rgb("#1a73e8")
#let scale = 1.5
#let items = ("alpha", "beta", "gamma")
#let kind = str
#let tab = "col1\tcol2\n"

#let card(title, body, width: 100%) = {
  block(width: width, inset: 8pt, stroke: 0.5pt + accent)[
    *#title* \
    #body
  ]
}

#let fib(n) = {
  assert(type(n) == int, message: "expected an \"int\", got\t\u{1F600}")
  if n <= 1 {
    return n
  } else {
    return fib(n - 1) + fib(n - 2)
  }
}

#show heading: it => {
  set text(fill: accent)
  it
}

= Introduction <intro>

This is _emphasised_ and *strong* text with a link to https://typst.app and
a reference to @intro. Escapes: \# \$ \* and a line break \
follows here.

== Lists

- First item
- Second item with `inline code`
+ Numbered item
/ Term: A definition

== Code

#for (i, item) in items.enumerate() [
  - Item #i is #item
]

#let total = 0
#while total < 3 {
  total += 1
}

#card("Fibonacci", [The tenth value is #fib(10).])
#card("Flags", [#true #false #none #auto])

```python
def hello():
    print("hello")
```

```
plain raw block
```

== Math

The area is $A = pi r^2$ and $sum_(k=0)^n k = (n (n + 1)) / 2$.

$ integral_0^1 x^2 dif x = 1 / 3 $

#figure(
  image("diagram.png", width: 60%),
  caption: [A diagram of #lorem(5)],
) <fig-diagram>

#include "appendix.typ"
