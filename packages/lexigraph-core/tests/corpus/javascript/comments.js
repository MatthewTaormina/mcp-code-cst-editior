// Comments at every legal position.
/* leading block */
import /* inside import */ { foo } from 'bar'; // trailing line

/** JSDoc on class */
class A /* between name and body */ {
  /** field doc */
  x = 1; // trailing

  // method comment
  m(/* param comment */ y /* between param and ) */) /* before body */ {
    /* first stmt */
    return y + this.x; // trailing return
  }
}

const a = new A(); /* trailing var decl */

// nested template literal with expressions and a comment-looking string
const s = `outer ${/* inline expr comment */ a.m(2)} // not a comment`;

// final dangling comment
