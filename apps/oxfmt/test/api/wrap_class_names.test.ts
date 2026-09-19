import { mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { format } from "../../dist/index.js";

const LONG_CLASSES =
  "bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30 dark:border-neutral-500/30 rounded-xl px-4 py-4";

describe("Wrap class names", () => {
  it("should wrap a long className to the print width", async () => {
    const input = `const A = <div className="${LONG_CLASSES}">Hello</div>;`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      wrapClassNames: true,
    });

    expect(result.code).toMatchInlineSnapshot(`
      "const A = (
        <div
          className="bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30
            dark:border-neutral-500/30 rounded-xl px-4 py-4"
        >
          Hello
        </div>
      );
      "
    `);
    expect(result.errors).toStrictEqual([]);
  });

  it("should be idempotent (double format)", async () => {
    const input = `const A = <div className="${LONG_CLASSES}">Hello</div>;`;
    const options = { printWidth: 80, wrapClassNames: true };

    const once = await format("test.tsx", input, options);
    const twice = await format("test.tsx", once.code, options);

    expect(twice.code).toBe(once.code);
    expect(twice.errors).toStrictEqual([]);
  });

  it("should NOT wrap when wrapClassNames is disabled (default)", async () => {
    const input = `const A = <div className="${LONG_CLASSES}">Hello</div>;`;

    const result = await format("test.tsx", input, { printWidth: 80 });

    expect(result.code).toContain(`className="${LONG_CLASSES}"`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should convert a long plain string argument to a wrapped template", async () => {
    const input = `const A = clsx("${LONG_CLASSES}");`;

    const result = await format("test.ts", input, {
      printWidth: 80,
      wrapClassNames: { functions: ["clsx"] },
    });

    expect(result.code).toMatchInlineSnapshot(`
      "const A = clsx(
        \`bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30
        dark:border-neutral-500/30 rounded-xl px-4 py-4\`,
      );
      "
    `);
    expect(result.errors).toStrictEqual([]);
  });

  it("should normalize a short template argument back to a quoted string", async () => {
    const input = "const A = clsx(`flex items-center gap-2`);";

    const result = await format("test.ts", input, {
      printWidth: 80,
      wrapClassNames: { functions: ["clsx"] },
    });

    expect(result.code).toBe(`const A = clsx("flex items-center gap-2");\n`);
    expect(result.errors).toStrictEqual([]);
  });

  it("delimiter conversion is idempotent in both directions", async () => {
    const options = { printWidth: 80, wrapClassNames: { functions: ["clsx"] } };
    for (const input of [
      `const A = clsx("${LONG_CLASSES}");`,
      "const A = clsx(`flex items-center gap-2`);",
    ]) {
      const once = await format("test.ts", input, options);
      const twice = await format("test.ts", once.code, options);
      expect(twice.code).toBe(once.code);
    }
  });

  it("should wrap a template literal in a configured tag context", async () => {
    const input = `const A = tw\`${LONG_CLASSES}\`;`;

    const result = await format("test.ts", input, {
      printWidth: 80,
      wrapClassNames: { functions: ["tw"] },
    });

    expect(result.code).toMatchInlineSnapshot(`
      "const A = tw\`bg-gray-100/50 dark:bg-neutral-900/50 border border-zinc-400/30
      dark:border-neutral-500/30 rounded-xl px-4 py-4\`;
      "
    `);
    expect(result.errors).toStrictEqual([]);
  });

  it("should sort first and then wrap when combined with sortTailwindcss", async () => {
    // Unsorted: p-4 before flex; sorter also removes the duplicate `flex`
    const input = `const A = <div className="p-4 flex flex text-white items-center justify-center rounded-md border border-zinc-400/30 bg-red-500">Hello</div>;`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      sortTailwindcss: true,
      wrapClassNames: true,
    });

    // Sorted order: flex before p-4, duplicate removed
    expect(result.code).toContain('className="flex');
    expect(result.code).not.toContain("flex flex");
    // Wrapped: the attribute value spans multiple lines
    const attr = result.code.match(/className="([^"]*)"/s)?.[1];
    expect(attr).toBeDefined();
    expect(attr).toContain("\n");
    expect(result.errors).toStrictEqual([]);
  });

  it("should NOT sort strings targeted only by wrapClassNames", async () => {
    const input = `const a = cn("p-4 flex items-center");\nconst b = clsx("p-4 flex items-center");\n`;

    const result = await format("test.tsx", input, {
      sortTailwindcss: { functions: ["cn"] },
      wrapClassNames: { functions: ["clsx"] },
    });

    expect(result.code).toBe(
      `const a = cn("flex items-center p-4");\nconst b = clsx("p-4 flex items-center");\n`,
    );
    expect(result.errors).toStrictEqual([]);
  });

  it("should turn a wrapped attribute into an expression with syntaxTransformation", async () => {
    const input = `const A = <div className="${LONG_CLASSES}">Hello</div>;`;
    const options = { printWidth: 60, wrapClassNames: { syntaxTransformation: true } };

    const result = await format("test.tsx", input, options);

    expect(result.code).toContain("className={`");
    expect(result.code).toContain("`}");
    // Short values keep their quoted form
    const short = await format("test.tsx", `const A = <div className="flex gap-2">Hello</div>;`, options);
    expect(short.code).toBe(`const A = <div className="flex gap-2">Hello</div>;\n`);
    expect(result.errors).toStrictEqual([]);
  });

  it("should wrap class names inside a Vue script block", async () => {
    const input = `<script setup>\nimport clsx from "clsx";\nconst cls = clsx("${LONG_CLASSES}");\n</script>\n<template> <div>x</div> </template>\n`;

    const result = await format("a.vue", input, {
      printWidth: 60,
      wrapClassNames: { functions: ["clsx"] },
    });

    expect(result.code).toContain("clsx(");
    // The class string wrapped inside the <script> block
    const wrapped = result.code.match(/clsx\(([\s\S]*?)\);/)?.[1];
    expect(wrapped).toContain("\n");
    for (const line of wrapped!.split("\n")) {
      expect(line.length).toBeLessThanOrEqual(60);
    }
    expect(result.errors).toStrictEqual([]);
  });

  it("should NOT wrap strings covered by preserveWhitespace", async () => {
    const input = `const A = <div className="  ${LONG_CLASSES}  ">Hello</div>;`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      sortTailwindcss: { preserveWhitespace: true },
      wrapClassNames: true,
    });

    const attr = result.code.match(/className="([^"]*)"/s)?.[1];
    expect(attr).toBeDefined();
    expect(attr).not.toContain("\n");
    expect(result.errors).toStrictEqual([]);
  });

  it("should wrap custom attributes when configured", async () => {
    const input = `const A = <Widget myClassProp="${LONG_CLASSES}">Hello</Widget>;`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      wrapClassNames: { attributes: ["myClassProp"] },
    });

    const attr = result.code.match(/myClassProp="([^"]*)"/s)?.[1];
    expect(attr).toBeDefined();
    expect(attr).toContain("\n");
    expect(result.errors).toStrictEqual([]);
  });

  it("should honor jsxSingleQuote when wrapping", async () => {
    const input = `const A = <div className="${LONG_CLASSES}">Hello</div>;`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      wrapClassNames: true,
      jsxSingleQuote: true,
    });

    const attr = result.code.match(/className='([^']*)'/s)?.[1];
    expect(attr).toBeDefined();
    expect(attr).toContain("\n");
    expect(result.errors).toStrictEqual([]);
  });

  it("should unwrap when the wrapped content fits on one line again", async () => {
    const input = `const A = (
  <div className="flex items-center gap-2
    px-4 py-4">
    Hello
  </div>
);
`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      wrapClassNames: true,
    });

    expect(result.code).toBe(
      `const A = <div className="flex items-center gap-2 px-4 py-4">Hello</div>;\n`,
    );
    expect(result.errors).toStrictEqual([]);
  });

  it("avoids the attribute-string newline with syntaxTransformation", async () => {
    const input = `const A = <div className="${LONG_CLASSES}">Hello</div>;`;

    const result = await format("test.tsx", input, {
      printWidth: 80,
      wrapClassNames: { syntaxTransformation: true },
    });

    // The wrapped value is a template literal, so no JSX attribute string
    // carries the newline (see the note above the hydration test).
    expect(result.code).toContain("className={`");
    expect(result.code).not.toMatch(/className="[^"]*\n/);
    expect(result.errors).toStrictEqual([]);
  });

  // The wrap newline becomes part of the runtime className string, the same as
  // prettier-plugin-classnames. That is what the React hydration reports are
  // about (facebook/react#35481, prettier-plugin-classnames#113): Babel
  // collapses a newline inside a JSX attribute string, SWC keeps it, so a build
  // mixing both (the React Compiler) hydrates with a mismatch. A formatter can
  // guarantee the two things pinned here by executing the formatted JSX: the
  // class TOKENS are untouched (CSS behavior identical) and the output is
  // deterministic. `syntaxTransformation` avoids the mismatch entirely by
  // printing a template literal, which both transforms keep as written.
  it("preserves runtime className tokens when wrapping (hydration safety)", async () => {
    const input = `export const A = <div className="${LONG_CLASSES}">Hello</div>;`;
    const options = { printWidth: 80, wrapClassNames: true };

    const result = await format("test.tsx", input, options);
    expect(result.errors).toStrictEqual([]);

    // Execute the formatted JSX (vitest transforms the .tsx import) with a
    // capturing jsx factory to observe the actual runtime className value.
    const dir = join(import.meta.dirname, "__temp__");
    await mkdir(dir, { recursive: true });
    const file = join(dir, "wrap-class-names-hydration.tsx");
    await writeFile(
      file,
      `/** @jsxRuntime classic */
/** @jsx h */
function h(_type: unknown, props: { className: string }) {
  return props;
}
${result.code}`,
    );

    try {
      const mod = await import(file);
      const runtime: string = mod.A.className;

      // Only whitespace may change: no class added, removed, or reordered.
      expect(runtime.split(/\s+/)).toStrictEqual(LONG_CLASSES.split(/\s+/));
      // The exact string is NOT the original — it now embeds the wrap newline.
      expect(runtime).toContain("\n");
      // Determinism: a second, independent format of the same source yields
      // byte-identical output — server and client builds sharing the formatted
      // source produce the same hydration string.
      const again = await format("test.tsx", input, options);
      expect(again.code).toBe(result.code);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});
