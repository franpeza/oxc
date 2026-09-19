// The closing quote counts toward the last line's width: at print width 80
// the last class moves down instead of ending the line at column 81.
const a = (
  <button type="button" className="text-sm text-sm text-sm text-sm text-sm text-sm text-sm text-sm text-sm px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 abcdef">
    x
  </button>
);

// Same for the closing backtick of a template.
const b = clsx(`text-sm text-sm text-sm text-sm text-sm text-sm text-sm text-sm text-sm px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 px-3 abcdef`);
