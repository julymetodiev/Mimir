export interface Shape {
  area(): number;
  name: string;
}
export type Id = string;
export enum Color {
  Red,
  Green,
}
export const LIMIT = 3;
export class Base {
  hello(): void {}
}
