// String literal types never wrap: a template literal type is a different type
const t = clsx(value as "inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background");
const u = clsx("inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background" satisfies string);
