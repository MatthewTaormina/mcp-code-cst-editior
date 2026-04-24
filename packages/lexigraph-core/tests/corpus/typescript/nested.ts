type Deep = {
  a: { b: { c: { d: { e: { f: { g: string } } } } } };
  list: Array<Array<Array<Array<number>>>>;
};

const x: Deep = {
  a: { b: { c: { d: { e: { f: { g: 'leaf' } } } } } },
  list: [[[[1, 2, 3]]]],
};

export default x;
