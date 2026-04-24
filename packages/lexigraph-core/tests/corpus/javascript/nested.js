function a(x) {
  return ((x + 1) * (x - 1)) / (x ** 2 - 1);
}

const tree = [
  [1, [2, [3, [4, [5, [6, [7, [8, [9, [10]]]]]]]]]],
  { a: { b: { c: { d: { e: { f: { g: 'deep' } } } } } } },
];

if (a(1) > 0) {
  if (a(2) > 0) {
    if (a(3) > 0) {
      if (a(4) > 0) {
        console.log(tree);
      }
    }
  }
}
