export interface Options {
  timeout: number;
  readonly name?: string;
  run(): void;
}

type Point = {
  x: number;
  y: number;
};

enum Direction {
  Up,
  Down = 2,
  "Left",
}

abstract class Base {
  protected id: number = 0;
  abstract describe(): string;
}

class Service extends Base {
  private retries = 3;
  static readonly instances: number;

  constructor(private readonly options: Options) {
    super();
  }

  describe(): string {
    return "service";
  }
}

export const DEFAULT_TIMEOUT = 30;
let counter: number = 0;
const make = (o: Options): Service => new Service(o);

function run(p: Point): number {
  const local = p.x;
  return local;
}
