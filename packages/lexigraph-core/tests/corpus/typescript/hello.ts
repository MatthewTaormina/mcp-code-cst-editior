// hello.ts
interface Greeter {
  greet(name: string): string;
}

export const g: Greeter = {
  greet(name) {
    return `hello, ${name}`;
  },
};
