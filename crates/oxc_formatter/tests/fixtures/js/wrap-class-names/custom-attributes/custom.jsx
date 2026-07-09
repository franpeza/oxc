// Custom attribute wraps only when configured via `attributes`
const a = (
  <Widget myClassProp="bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30 dark:border-neutral-500/30 rounded-xl px-4 py-4">
    content
  </Widget>
);

// Default targets always wrap
const b = (
  <div class="inline-flex items-center justify-center rounded-md text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none">
    content
  </div>
);
