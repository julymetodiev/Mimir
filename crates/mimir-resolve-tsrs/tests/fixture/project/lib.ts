import { Base, Shape } from "./types";

export class Circle extends Base implements Shape {
  name = "c";
  private r = 1;
  constructor(r: number) {
    super();
    this.r = r;
  }
  area(): number {
    return helper(this.r);
  }
  get size(): number {
    return this.r;
  }
  set size(v: number) {
    this.r = v;
  }
  #secret(): number {
    return 1;
  }
  reveal(): number {
    return this.#secret();
  }
}

export class Plain {}

export class Square {
  area(): number {
    return 2;
  }
}

export function helper(n: number): number {
  return n;
}

export function over(a: string): void;
export function over(a: number): void;
export function over(a: unknown): void {}

export const arrow = (x: number): number => x;

export function decorate(target: unknown): void {}
