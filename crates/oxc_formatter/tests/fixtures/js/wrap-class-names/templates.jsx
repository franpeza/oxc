// Tagged template with configured tag wraps (never converted: tag needs a template)
const a = tw`bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30 dark:border-neutral-500/30 rounded-xl px-4 py-4`;

// Template literal argument of a configured function wraps
const b = clsx(`inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors`);

// Template with expressions: classes touching ${} boundaries stay put
const c = clsx(`${base}-header items-center justify-between gap-4 border-b border-neutral-200 px-6 py-4 text-lg font-semibold ${active ? "is-active" : ""}`);

// Delimiter conversion: long plain string argument converts to a wrapped
// backtick template (a quoted string cannot contain a literal newline)
const d = clsx("inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors");

// Delimiter conversion, other direction: a template that fits on one line
// normalizes to a quoted string
const e = clsx(`flex items-center gap-2`);

// Short strings keep their quoted form; nested non-class call is untouched
const f = clsx("flex gap-2", x.includes("do not touch this string content here") ? "p-2" : "p-4");

// Expression container in JSX: same conversion rules
const g = <div className={`flex items-center gap-2`}>short</div>;
const h = <div className={"inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors"}>long</div>;

// Logical/ternary operands convert too
const i = <div className={cond && "inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors"}>x</div>;

// Not convertible: content with template collisions keeps its source form
const j = clsx("flex gap-2 before:content-['${'] after:content-['\\2713']");
