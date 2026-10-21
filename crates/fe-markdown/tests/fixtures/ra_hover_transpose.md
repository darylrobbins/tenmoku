
```rust
core::option::Option
```

```rust
impl<T, E> Option<Result<T, E>>
pub const fn transpose(self) -> Result<Option<T>, E>
```

---

`T` = `u8`, `E` = `String`

---

Transposes an `Option` of a [`Result`](https://doc.rust-lang.org/stable/core/result/enum.Result.html) into a [`Result`](https://doc.rust-lang.org/stable/core/result/enum.Result.html) of an `Option`.

<code>
[Some](https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.Some)([Ok](https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Ok)(\_))</code> is mapped to <code>
[Ok](https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Ok)([Some](https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.Some)(\_))</code>,
<code>
[Some](https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.Some)([Err](https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Err)(\_))</code> is mapped to <code>
[Err](https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Err)(\_)</code>,
and [`None`](https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.None) will be mapped to <code>
[Ok](https://doc.rust-lang.org/stable/core/result/enum.Result.html#variant.Ok)([None](https://doc.rust-lang.org/stable/core/option/enum.Option.html#variant.None))</code>.

# Examples

```rust
#[derive(Debug, Eq, PartialEq)]
struct SomeErr;

let x: Option<Result<i32, SomeErr>> = Some(Ok(5));
let y: Result<Option<i32>, SomeErr> = Ok(Some(5));
assert_eq!(x.transpose(), y);
```
