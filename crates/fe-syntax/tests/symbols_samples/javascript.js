import { readFile } from "fs";

export class Store {
  count = 0;
  #secret = "s";
  static instances = 1;

  constructor() {
    this.items = [];
  }

  add(item) {
    this.items.push(item);
  }
}

const routes = ["/", "/about"];
let current = null;
var legacy = 1;
export const timeout = 30;
export let mode = "dev";
const { a, b } = { a: 1, b: 2 };

const handler = () => current;

function main() {
  const local = 1;
  return local + timeout;
}
