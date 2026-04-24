// Awkward whitespace, template literals, and regex.
const r =/regex with \/escape/gi   ;
   const   tag    =   String   .   raw   `multi
line
with     irregular   spacing
${1 + 2}
`;

const obj   =   {
   a   :   1   ,
   b   :   [   1   ,    2,3,    4,]    ,
   "c": `nested ${`inner ${1}`}`,
};

const fn = (...args) =>
   args
     .map((x) =>
       x * 2,
     )
     .filter((x) => x > 0);
