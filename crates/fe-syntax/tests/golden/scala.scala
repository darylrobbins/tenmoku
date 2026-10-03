// A small expression evaluator, used as a golden highlighting sample.
package golden.calc

import scala.collection.mutable
import scala.util.{Failure, Success, Try}

/** An arithmetic expression over named variables. */
sealed trait Expr
final case class Num(value: Double) extends Expr
final case class Var(name: String) extends Expr
final case class BinOp(op: Char, left: Expr, right: Expr) extends Expr

enum Mode:
  case Strict, Lenient

object Eval {
  val MaxDepth: Int = 64
  private lazy val cache = mutable.Map.empty[Expr, Double]

  type Env = Map[String, Double]

  def apply(expr: Expr, env: Env, mode: Mode = Mode.Strict): Either[String, Double] =
    Try(eval(expr, env, 0)) match {
      case Success(v) if !v.isNaN => Right(v)
      case Success(_) => Left("not a number")
      case Failure(e) if mode == Mode.Lenient => Right(0.0)
      case Failure(e) => Left(s"failed: ${e.getMessage}")
    }

  private def eval(expr: Expr, env: Env, depth: Int): Double = {
    require(depth < MaxDepth, "too deep")
    expr match {
      case Num(v) => v
      case Var(name) => env.getOrElse(name, throw new NoSuchElementException(name))
      case BinOp('+', l, r) => eval(l, env, depth + 1) + eval(r, env, depth + 1)
      case BinOp('*', l, r) => eval(l, env, depth + 1) * eval(r, env, depth + 1)
      case BinOp(op, _, _) => throw new IllegalArgumentException(s"unknown op $op")
    }
  }

  def memo(expr: Expr, env: Env): Double =
    cache.getOrElseUpdate(expr, eval(expr, env, 0))
}

abstract class Parser[A](input: String) {
  protected var pos = 0
  def parse(): Option[A]

  protected def peek: Option[Char] =
    if (pos < input.length) Some(input.charAt(pos)) else None
}

class NumberParser(input: String) extends Parser[Expr](input) {
  override def parse(): Option[Expr] = {
    val start = pos
    while (peek.exists(c => c.isDigit || c == '.')) pos += 1
    input.substring(start, pos).toDoubleOption.map(Num(_))
  }
}

@main def run(args: String*): Unit = {
  val env: Eval.Env = Map("x" -> 2.0, "y" -> 0x10)
  val expr = BinOp('+', Num(1.5e1), BinOp('*', Var("x"), Var("y")))
  for (n <- 1 to 3 if n % 2 == 1) {
    println(s"$n: ${Eval(expr, env)}")
  }
  val ok = args.nonEmpty && args.forall(_.nonEmpty)
  println(if ok then "args" else null)
}
