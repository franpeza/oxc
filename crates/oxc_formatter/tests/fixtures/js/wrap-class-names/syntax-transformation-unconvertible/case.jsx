// Content that cannot become a template (a backtick, a `${` or a backslash
// would change the value) keeps its quoted form and wraps in place, where a
// JSX attribute string may hold newlines. `b` shows the converted form.
const a = <div className="flex gap-2 before:content-['${'] after:content-['x'] items-center">y</div>;
const b = <div className="flex gap-2 aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj">y</div>;

// JSX decodes an HTML character reference in an attribute string but not in a
// template, so such a string keeps its quotes and wraps in place.
const c = <div className="x&amp;y c00 c01 c02 c03 c04 c05 c06 c07 c08 c09 c10 c11 c12 c13 c14 c15 c16 c17 c18 c19 c20 c21">y</div>;
// A Tailwind arbitrary variant is not a character reference and still converts.
const d = <div className="[&_svg]:size-4 c00 c01 c02 c03 c04 c05 c06 c07 c08 c09 c10 c11 c12 c13 c14 c15 c16 c17 c18 c19 c20 c21">y</div>;
