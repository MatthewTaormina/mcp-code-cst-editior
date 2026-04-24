// generics, conditional types, tuples, comments throughout.
/** Toplevel doc */
type If<C extends boolean, T, F> = C extends true ? T : F /* trailing */;

type Pair<A, B = A /* default */> = readonly [A, B];

class Stack<T> /* between class name and body */ {
  private items: T[] = []; // trailing

  push(/* item */ item: T): void {
    this.items.push(item); /* before semi */
  }

  pop(): T | undefined {
    return this.items.pop();
  }
}

const s = new Stack<number>();
s.push(1);
s.push(2);
const top: number | undefined = s.pop();
