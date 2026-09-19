// Already-wrapped input (as this formatter would emit it) must re-format
// to the same output: the embedded newlines/indentation collapse and the
// fill re-wraps identically. Also pins the opening-element layout: a
// multiline wrap-target value must not flip the single-attribute layout.
const a = (
  <div
    className="bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30
      dark:border-neutral-500/30 rounded-xl px-4 py-4"
  >
    content
  </div>
);

const b = tw`bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30
  dark:border-neutral-500/30 rounded-xl px-4 py-4`;
