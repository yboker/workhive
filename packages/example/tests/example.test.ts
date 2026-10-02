import { greet } from "../index";

if (greet("world") !== "Hello, world!") {
  throw new Error("greet should return a greeting");
}
