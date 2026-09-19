// Long className wraps to print width
const a = (
  <div className="bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30 dark:border-neutral-500/30 rounded-xl px-4 py-4">
    content
  </div>
);

// Short className stays on one line
const b = <div className="flex items-center gap-2">content</div>;

// Single class longer than remaining width: no split, overflows
const c = (
  <div className="an-extremely-long-single-utility-class-name-that-cannot-be-split-anywhere-at-all">
    content
  </div>
);

// Empty and whitespace-only values
const d = <div className="">content</div>;
const e = <div className="   ">content</div>;

// Non-target attribute untouched
const f = (
  <div title="a very long tooltip text that would otherwise wrap if it were a class attribute but it is not one">
    content
  </div>
);

// Multi-attribute element: attributes break to own lines, string wraps within
const g = (
  <button type="button" disabled className="inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none">
    click
  </button>
);

// Deeply nested JSX with little remaining width
function Nested() {
  return (
    <div>
      <section>
        <article>
          <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
            content
          </div>
        </article>
      </section>
    </div>
  );
}
