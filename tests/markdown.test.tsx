import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { MarkdownBody, collectInlineText, classifyHtmlTag } from "../src/lib/markdown";
import { createDetailsController, DetailsCtx } from "../src/lib/details-store";

/** Render markdown source and return the captured character frame. */
async function renderMarkdown(source: string, width = 60, height = 30): Promise<string> {
  const { renderOnce, captureCharFrame } = await testRender(
    () => <MarkdownBody source={source} />,
    { width, height },
  );
  await renderOnce();
  return captureCharFrame();
}

describe("MarkdownBody — block nodes", () => {
  test("renders heading with # prefix", async () => {
    const frame = await renderMarkdown("## Hello World");
    expect(frame).toContain("## Hello World");
  });

  test("renders h1 through h3 with correct prefix depth", async () => {
    const frame = await renderMarkdown("# H1\n\n## H2\n\n### H3");
    expect(frame).toContain("# H1");
    expect(frame).toContain("## H2");
    expect(frame).toContain("### H3");
  });

  test("renders paragraph text", async () => {
    const frame = await renderMarkdown("Hello, this is a paragraph.");
    expect(frame).toContain("Hello, this is a paragraph.");
  });

  test("renders fenced code block with language label", async () => {
    const frame = await renderMarkdown("```ts\nconst x = 1;\n```");
    expect(frame).toContain("const x = 1;");
    expect(frame).toContain("```ts");
  });

  test("renders fenced code block without language", async () => {
    const frame = await renderMarkdown("```\nhello\n```");
    expect(frame).toContain("hello");
    // No language label when lang is empty
    expect(frame).not.toMatch(/```[a-z]/);
  });

  test("renders unordered list with bullets", async () => {
    const frame = await renderMarkdown("- item 1\n- item 2\n- item 3");
    expect(frame).toContain("• item 1");
    expect(frame).toContain("• item 2");
    expect(frame).toContain("• item 3");
  });

  test("renders ordered list with numbers", async () => {
    const frame = await renderMarkdown("1. first\n2. second\n3. third");
    expect(frame).toContain("1. first");
    expect(frame).toContain("2. second");
    expect(frame).toContain("3. third");
  });

  test("renders nested list with indentation", async () => {
    const frame = await renderMarkdown("- outer\n  - inner\n    - deep");
    expect(frame).toContain("• outer");
    expect(frame).toContain("• inner");
    expect(frame).toContain("• deep");
  });

  test("renders blockquote with │ prefix", async () => {
    const frame = await renderMarkdown("> This is a quote");
    expect(frame).toContain("│ This is a quote");
  });

  test("renders thematic break as horizontal rule", async () => {
    const frame = await renderMarkdown("above\n\n---\n\nbelow");
    expect(frame).toContain("above");
    expect(frame).toContain("────");
    expect(frame).toContain("below");
  });

  test("renders multi-section document", async () => {
    const source = [
      "# Title",
      "",
      "Some intro text.",
      "",
      "## Section",
      "",
      "- bullet 1",
      "- bullet 2",
      "",
      "```js",
      "console.log('hi');",
      "```",
      "",
      "> A wise quote.",
    ].join("\n");
    const frame = await renderMarkdown(source, 80, 40);
    expect(frame).toContain("# Title");
    expect(frame).toContain("Some intro text.");
    expect(frame).toContain("## Section");
    expect(frame).toContain("• bullet 1");
    expect(frame).toContain("console.log('hi');");
    expect(frame).toContain("│ A wise quote.");
  });

  test("hides HTML comments (e.g. bot badges)", async () => {
    const source = [
      "Real content above.",
      "",
      "<!-- devin-review-badge-begin -->",
      "",
      "<!-- devin-review-badge-end -->",
    ].join("\n");
    const frame = await renderMarkdown(source);
    expect(frame).toContain("Real content above.");
    expect(frame).not.toContain("[html content]");
    expect(frame).not.toContain("devin");
  });

  test("renders non-comment HTML as placeholder", async () => {
    const source = ["Before.", "", "<div>some widget</div>"].join("\n");
    const frame = await renderMarkdown(source);
    expect(frame).toContain("Before.");
    expect(frame).toContain("[<div>]");
  });

  test("renders empty source without error", async () => {
    const frame = await renderMarkdown("");
    // Should just be whitespace — no crash
    expect(frame).toBeDefined();
  });
});

describe("MarkdownBody — details/summary", () => {
  const detailsSource = [
    "Some text.",
    "",
    "<details>",
    "<summary>Click to expand</summary>",
    "",
    "Hidden **bold** content.",
    "",
    "</details>",
    "",
    "After details.",
  ].join("\n");

  test("renders collapsed summary with arrow", async () => {
    const frame = await renderMarkdown(detailsSource);
    expect(frame).toContain("\u25b6"); // right arrow
    expect(frame).toContain("Click to expand");
    // Content is hidden when collapsed
    expect(frame).not.toContain("Hidden");
    expect(frame).not.toContain("bold");
    // Surrounding text still renders
    expect(frame).toContain("Some text.");
    expect(frame).toContain("After details.");
  });

  test("does not show [html content] for details blocks", async () => {
    const frame = await renderMarkdown(detailsSource);
    expect(frame).not.toContain("[html content]");
  });

  test("click toggles expansion", async () => {
    const { renderOnce, captureCharFrame, mockMouse } = await testRender(
      () => <MarkdownBody source={detailsSource} />,
      { width: 60, height: 30 },
    );
    await renderOnce();
    let frame = captureCharFrame();
    expect(frame).toContain("\u25b6"); // collapsed
    expect(frame).not.toContain("Hidden");

    // Find the summary row and click it
    const lines = frame.split("\n");
    const summaryLine = lines.findIndex((l) => l.includes("Click to expand"));
    expect(summaryLine).toBeGreaterThanOrEqual(0);
    await mockMouse.click(2, summaryLine);
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("\u25bc"); // expanded arrow
    expect(frame).toContain("Hidden");
    expect(frame).toContain("bold");

    // Click again to collapse
    const lines2 = frame.split("\n");
    const summaryLine2 = lines2.findIndex((l) => l.includes("Click to expand"));
    await mockMouse.click(2, summaryLine2);
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("\u25b6"); // collapsed again
    expect(frame).not.toContain("Hidden");
  });

  test("DetailsCtx.toggleAll expands all and collapses all", async () => {
    const twoDetails = [
      "<details>",
      "<summary>First</summary>",
      "",
      "Content A.",
      "",
      "</details>",
      "",
      "<details>",
      "<summary>Second</summary>",
      "",
      "Content B.",
      "",
      "</details>",
    ].join("\n");

    const ctrl = createDetailsController();
    const { renderOnce, captureCharFrame } = await testRender(
      () => (
        <DetailsCtx.Provider value={ctrl}>
          <MarkdownBody source={twoDetails} />
        </DetailsCtx.Provider>
      ),
      { width: 60, height: 30 },
    );
    await renderOnce();
    let frame = captureCharFrame();
    expect(frame).not.toContain("Content A.");
    expect(frame).not.toContain("Content B.");

    // toggleAll should expand all
    ctrl.toggleAll();
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("Content A.");
    expect(frame).toContain("Content B.");

    // toggleAll again should collapse all (all were expanded)
    ctrl.toggleAll();
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).not.toContain("Content A.");
    expect(frame).not.toContain("Content B.");
  });

  test("toggleAll expands all when some are collapsed", async () => {
    const twoDetails = [
      "<details>",
      "<summary>First</summary>",
      "",
      "Content A.",
      "",
      "</details>",
      "",
      "<details>",
      "<summary>Second</summary>",
      "",
      "Content B.",
      "",
      "</details>",
    ].join("\n");

    const ctrl = createDetailsController();
    const { renderOnce, captureCharFrame, mockMouse } = await testRender(
      () => (
        <DetailsCtx.Provider value={ctrl}>
          <MarkdownBody source={twoDetails} />
        </DetailsCtx.Provider>
      ),
      { width: 60, height: 30 },
    );
    await renderOnce();

    // Click to expand only the first one
    const lines = captureCharFrame().split("\n");
    const firstLine = lines.findIndex((l) => l.includes("First"));
    await mockMouse.click(2, firstLine);
    await renderOnce();
    let frame = captureCharFrame();
    expect(frame).toContain("Content A.");
    expect(frame).not.toContain("Content B.");

    // toggleAll: some collapsed → expand all
    ctrl.toggleAll();
    await renderOnce();
    frame = captureCharFrame();
    expect(frame).toContain("Content A.");
    expect(frame).toContain("Content B.");
  });

  test("defaults to 'Details' when no summary tag", async () => {
    const source = ["<details>", "", "Secret content.", "", "</details>"].join("\n");
    const frame = await renderMarkdown(source);
    expect(frame).toContain("\u25b6");
    expect(frame).toContain("Details");
    expect(frame).not.toContain("Secret content.");
  });
});

describe("MarkdownBody — inline nodes", () => {
  test("renders bold text", async () => {
    const frame = await renderMarkdown("Some **bold** text.");
    expect(frame).toContain("Some");
    expect(frame).toContain("bold");
    expect(frame).toContain("text.");
  });

  test("renders italic text", async () => {
    const frame = await renderMarkdown("Some *italic* text.");
    expect(frame).toContain("Some");
    expect(frame).toContain("italic");
    expect(frame).toContain("text.");
  });

  test("renders inline code", async () => {
    const frame = await renderMarkdown("Use `foo()` here.");
    expect(frame).toContain("Use");
    expect(frame).toContain("foo()");
    expect(frame).toContain("here.");
  });

  test("renders link as clickable text (no raw URL)", async () => {
    const frame = await renderMarkdown("See [docs](https://example.com) for info.");
    expect(frame).toContain("docs");
    // URL is an OSC 8 hyperlink, not visible in the character frame
    expect(frame).not.toContain("https://example.com");
  });

  test("renders image as alt text placeholder", async () => {
    const frame = await renderMarkdown("![screenshot](https://example.com/img.png)");
    expect(frame).toContain("[image: screenshot]");
  });

  test("renders nested bold inside italic", async () => {
    const frame = await renderMarkdown("*italic and **bold** inside*");
    expect(frame).toContain("italic and");
    expect(frame).toContain("bold");
    expect(frame).toContain("inside");
  });

  test("renders mixed inline in list items", async () => {
    const frame = await renderMarkdown("- Use `code` and **bold** in lists");
    expect(frame).toContain("code");
    expect(frame).toContain("bold");
  });

  test("renders link inside heading", async () => {
    const frame = await renderMarkdown("## See [API docs](https://api.example.com)");
    expect(frame).toContain("## See");
    expect(frame).toContain("API docs");
  });
});

describe("MarkdownBody — inline HTML", () => {
  test("drops known inline tags from Graphite badge", async () => {
    const source =
      "*Spotted by [Graphite](https://example.com)*" +
      "<i class='graphite__hidden'><br /><br />" +
      '<a href="https://example.com">Fix in Graphite</a></i>' +
      "<i class='graphite__hidden'><br /><br />" +
      "Is this helpful?</i>";
    const frame = await renderMarkdown(source);
    expect(frame).toContain("Spotted by");
    expect(frame).toContain("Graphite");
    expect(frame).not.toContain("<i");
    expect(frame).not.toContain("</i>");
    expect(frame).not.toContain("<br");
    expect(frame).not.toContain("<a ");
    expect(frame).not.toContain("[html content]");
  });

  test("renders <br> as newline", async () => {
    const frame = await renderMarkdown("Hello<br />world");
    const lines = frame
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const helloLine = lines.findIndex((l) => l.includes("Hello"));
    const worldLine = lines.findIndex((l) => l.includes("world"));
    expect(helloLine).toBeGreaterThanOrEqual(0);
    expect(worldLine).toBeGreaterThan(helloLine);
    expect(frame).not.toContain("<br");
  });

  test("shows [<tag>] for unknown tags", async () => {
    const frame = await renderMarkdown("Hello<div>stuff</div>world");
    expect(frame).toContain("Hello");
    expect(frame).toContain("[<div>]");
  });

  test("text between ignored tags still renders", async () => {
    const frame = await renderMarkdown("before<i>italic text</i>after");
    expect(frame).toContain("before");
    expect(frame).toContain("italic text");
    expect(frame).toContain("after");
    expect(frame).not.toContain("<i>");
  });
});

describe("classifyHtmlTag", () => {
  test("ignores known inline tags", () => {
    expect(classifyHtmlTag("<i class='x'>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("<a href='x'>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("<img src='x'>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("<picture>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("<source media='x' srcset='y'>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("<span>")).toEqual({ kind: "ignore" });
  });

  test("classifies <br> as br", () => {
    expect(classifyHtmlTag("<br />")).toEqual({ kind: "br" });
    expect(classifyHtmlTag("<br>")).toEqual({ kind: "br" });
  });

  test("ignores closing tags", () => {
    expect(classifyHtmlTag("</i>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("</a>")).toEqual({ kind: "ignore" });
    expect(classifyHtmlTag("</span>")).toEqual({ kind: "ignore" });
  });

  test("classifies HTML comments", () => {
    expect(classifyHtmlTag("<!-- hello -->")).toEqual({ kind: "comment" });
  });

  test("returns unknown with tag name for block-level tags", () => {
    expect(classifyHtmlTag("<div>")).toEqual({ kind: "unknown", tag: "div" });
    expect(classifyHtmlTag("<table>")).toEqual({ kind: "unknown", tag: "table" });
  });
});

describe("collectInlineText", () => {
  test("extracts plain text", () => {
    expect(collectInlineText([{ type: "text", value: "hello" }])).toBe("hello");
  });

  test("extracts text from nested strong/emphasis", () => {
    const nodes = [
      { type: "text" as const, value: "a " },
      {
        type: "strong" as const,
        children: [{ type: "text" as const, value: "bold" }],
      },
      { type: "text" as const, value: " b" },
    ];
    expect(collectInlineText(nodes as any)).toBe("a bold b");
  });

  test("extracts inline code value", () => {
    const nodes = [{ type: "inlineCode" as const, value: "foo()" }];
    expect(collectInlineText(nodes as any)).toBe("foo()");
  });

  test("handles empty array", () => {
    expect(collectInlineText([])).toBe("");
  });
});
