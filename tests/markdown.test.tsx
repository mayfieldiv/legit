import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { MarkdownBody, collectInlineText } from "../src/lib/markdown";

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
		expect(frame).toContain("[html content]");
	});

	test("renders empty source without error", async () => {
		const frame = await renderMarkdown("");
		// Should just be whitespace — no crash
		expect(frame).toBeDefined();
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
