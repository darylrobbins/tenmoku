import { readFile } from "fs";

export class Greeter {
  constructor(name) {
    this.name = name;
  }

  greet() {
    return `hello ${this.name}`;
  }

  static create() {
    return new Greeter("x");
  }
}

function add(a, b) {
  return a + b;
}

function* count() {
  yield 1;
}

const square = (x) => x * x;
let cube = function (x) {
  return x * x * x;
};
var legacy = function named() {};

export const handler = async (event) => {
  const local = () => event;
  return local();
};

const value = 42;
