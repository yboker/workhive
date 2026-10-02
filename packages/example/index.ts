import { greeting } from "./lib/impl";

export function greet(name: string): string {
  return greeting(name);
}
