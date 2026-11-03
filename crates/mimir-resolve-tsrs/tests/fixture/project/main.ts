import { Circle, helper as renamed, over, arrow, Plain, helper, Square } from "./lib";
import * as ns from "./lib";
import { Color, Id, LIMIT, Shape } from "./types";

export function run(s: Shape, items: number[], id: Id): number {
  const c = new Circle(1);
  c.area();
  s.area();
  renamed(1);
  over("a");
  arrow(1);
  new Plain();
  ns.helper(2);
  const text = "ьь"; helper(3);
  items.map(renamed);
  return LIMIT + Color.Red;
}

export function accessors(c: Circle): void {
  c.size;
  c.size = 2;
}

export function both(c: Circle): void {
  c.size += 1;
}

export function fields(s: Shape): string {
  return s.name;
}

export function holder() {
  return { helper };
}

export function shadowed(helper: () => void): void {
  helper();
}

export function computed(c: Circle, name: "area"): void {
  c[name]();
}

export function either(x: Circle | Square): void {
  x.area();
}

export function chain(o: { inner: Circle | Square }): void {
  o.inner.area();
}
