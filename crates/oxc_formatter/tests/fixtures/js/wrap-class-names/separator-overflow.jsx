// The quoted form fits when the string itself does: a trailing `,` past the
// print width keeps the quotes, as in prettier-plugin-classnames.
const b = clsx("aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeeeeeee ffffffffff gggggggggg");

// A preserved boundary space counts toward the last line, next to the delimiter.
const a = clsx(x + " aaaaaaaaaa bbbbbbbbbb cccccccccc dddddddddd eeeeeeeeee ffffffffff " + y);
