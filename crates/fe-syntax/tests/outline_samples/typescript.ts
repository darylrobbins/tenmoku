export interface Shape {
  area(): number;
  name: string;
}

export type Pair = [number, number];

enum Color {
  Red,
  Green,
}

abstract class Base {
  abstract describe(): string;

  hello(): string {
    return "hello";
  }
}

class Circle extends Base implements Shape {
  name = "circle";

  constructor(private radius: number) {
    super();
  }

  area(): number {
    return Math.PI * this.radius ** 2;
  }

  describe(): string {
    return this.name;
  }
}

namespace Geometry {
  export function origin(): Pair {
    return [0, 0];
  }
}

declare function external(x: number): string;

export const makeCircle = (r: number): Circle => new Circle(r);

function overloaded(x: string): string;
function overloaded(x: number): number;
function overloaded(x: any): any {
  return x;
}
