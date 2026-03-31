import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { DetailView } from "../src/components/DetailView";
import type { PRDetail, CheckRun, FullReviewThread, IssueComment } from "../src/lib/types";
import { makePR } from "./helpers";

function makeDetail(overrides: Partial<PRDetail> = {}): PRDetail {
	return {
		...makePR(),
		body: "## Summary\n\nThis fixes the **bug**.",
		...overrides,
	};
}

async function renderDetail(
	props: Partial<Parameters<typeof DetailView>[0]> = {},
	width = 80,
	height = 40,
) {
	const defaults = {
		pr: makeDetail(),
		threads: [],
		comments: [],
		loading: false,
		showResolved: false,
		showBotComments: true,
	};
	const { renderOnce, captureCharFrame } = await testRender(
		() => <DetailView {...defaults} {...props} />,
		{ width, height },
	);
	await renderOnce();
	return captureCharFrame();
}

describe("DetailView", () => {
	test("shows loading state when no PR loaded", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => (
				<DetailView
					pr={undefined}
					threads={[]}
					comments={[]}
					loading={true}
					showResolved={false}
					showBotComments={true}
				/>
			),
			{ width: 80, height: 10 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toMatch(/loading/i);
	});

	test("renders PR header with number, title, and author", async () => {
		const frame = await renderDetail({
			pr: makeDetail({ number: 99, title: "Big refactor", author: "bob" }),
		});
		expect(frame).toContain("#99");
		expect(frame).toContain("Big refactor");
		expect(frame).toContain("bob");
	});

	test("renders PR description as markdown", async () => {
		const frame = await renderDetail({
			pr: makeDetail({ body: "## Heading\n\nSome text with `code`." }),
		});
		expect(frame).toContain("## Heading");
		expect(frame).toContain("Some text with");
		expect(frame).toContain("code");
	});

	test("shows 'No description' for empty body", async () => {
		const frame = await renderDetail({ pr: makeDetail({ body: "" }) });
		expect(frame).toContain("No description");
	});

	test("renders CI checks with pass/fail counts", async () => {
		const checks: CheckRun[] = [
			{ name: "build", status: "completed", conclusion: "success" },
			{ name: "lint", status: "completed", conclusion: "failure" },
			{ name: "deploy", status: "in_progress", conclusion: null },
		];
		const pr = { ...makeDetail(), checks } as any;
		const frame = await renderDetail({ pr });
		expect(frame).toContain("CI Checks");
		expect(frame).toContain("1/3 passed");
		expect(frame).toContain("1 failed");
		expect(frame).toContain("1 pending");
		expect(frame).toContain("build");
		expect(frame).toContain("lint");
		expect(frame).toContain("deploy");
	});

	test("does not render checks section when no checks", async () => {
		const frame = await renderDetail({ pr: makeDetail() });
		expect(frame).not.toContain("CI Checks");
	});

	test("renders draft badge", async () => {
		const frame = await renderDetail({ pr: makeDetail({ isDraft: true }) });
		expect(frame).toContain("draft");
	});

	test("renders size info", async () => {
		const frame = await renderDetail({
			pr: makeDetail({ additions: 100, deletions: 20 }),
		});
		expect(frame).toContain("+100");
		expect(frame).toContain("-20");
	});

	// ── Review Threads ─────────────────────────────────────────────────────

	test("renders unresolved review threads with file path and comments", async () => {
		const threads: FullReviewThread[] = [
			{
				id: "RT_1",
				isResolved: false,
				path: "src/foo.ts",
				line: 42,
				comments: [
					{
						id: "RC_1",
						author: "bob",
						body: "Needs a null check here",
						createdAt: "2026-03-10T00:00:00Z",
						url: "https://github.com/acme/widgets/pull/42#discussion_r1",
						isBot: false,
					},
					{
						id: "RC_2",
						author: "alice",
						body: "Good catch, fixing",
						createdAt: "2026-03-11T00:00:00Z",
						url: "https://github.com/acme/widgets/pull/42#discussion_r2",
						isBot: false,
					},
				],
			},
		];
		const frame = await renderDetail({ threads });
		expect(frame).toContain("Review Threads");
		expect(frame).toContain("1 shown");
		expect(frame).toContain("src/foo.ts:42");
		expect(frame).toContain("unresolved");
		expect(frame).toContain("bob");
		expect(frame).toContain("Needs a null check here");
		expect(frame).toContain("alice");
		expect(frame).toContain("Good catch, fixing");
	});

	test("hides resolved threads by default", async () => {
		const threads: FullReviewThread[] = [
			{
				id: "RT_1",
				isResolved: true,
				path: "src/bar.ts",
				line: 10,
				comments: [
					{
						id: "RC_1",
						author: "bob",
						body: "This was fixed",
						createdAt: "2026-03-10T00:00:00Z",
						url: "https://example.com",
						isBot: false,
					},
				],
			},
		];
		const frame = await renderDetail({ threads, showResolved: false });
		expect(frame).toContain("Review Threads");
		expect(frame).toContain("1 hidden");
		expect(frame).toContain("All threads resolved or hidden");
		expect(frame).not.toContain("This was fixed");
	});

	test("shows resolved threads when showResolved is true", async () => {
		const threads: FullReviewThread[] = [
			{
				id: "RT_1",
				isResolved: true,
				path: "src/bar.ts",
				line: null,
				comments: [
					{
						id: "RC_1",
						author: "bob",
						body: "This was fixed",
						createdAt: "2026-03-10T00:00:00Z",
						url: "https://example.com",
						isBot: false,
					},
				],
			},
		];
		const frame = await renderDetail({ threads, showResolved: true });
		expect(frame).toContain("resolved");
		expect(frame).toContain("This was fixed");
		expect(frame).toContain("src/bar.ts");
	});

	test("hides bot-only threads when showBotComments is false", async () => {
		const threads: FullReviewThread[] = [
			{
				id: "RT_1",
				isResolved: false,
				path: "src/bot.ts",
				line: 1,
				comments: [
					{
						id: "RC_1",
						author: "copilot[bot]",
						body: "Suggestion: refactor",
						createdAt: "2026-03-10T00:00:00Z",
						url: "https://example.com",
						isBot: true,
					},
				],
			},
		];
		const frame = await renderDetail({ threads, showBotComments: false });
		expect(frame).toContain("1 hidden");
		expect(frame).not.toContain("Suggestion: refactor");
	});

	test("does not render threads section when no threads exist", async () => {
		const frame = await renderDetail({ threads: [] });
		expect(frame).not.toContain("Review Threads");
	});

	test("shows file path without line when line is null", async () => {
		const threads: FullReviewThread[] = [
			{
				id: "RT_1",
				isResolved: false,
				path: "README.md",
				line: null,
				comments: [
					{
						id: "RC_1",
						author: "bob",
						body: "Update readme",
						createdAt: "2026-03-10T00:00:00Z",
						url: "https://example.com",
						isBot: false,
					},
				],
			},
		];
		const frame = await renderDetail({ threads });
		expect(frame).toContain("README.md");
		expect(frame).not.toContain("README.md:");
	});

	// ── Conversation ────────────────────────────────────────────────────────

	test("renders issue comments in conversation section", async () => {
		const comments: IssueComment[] = [
			{
				id: 100,
				author: "alice",
				body: "Looks good overall",
				createdAt: "2026-03-10T00:00:00Z",
				url: "https://github.com/acme/widgets/pull/42#issuecomment-100",
				isBot: false,
			},
			{
				id: 101,
				author: "bob",
				body: "Thanks for the review",
				createdAt: "2026-03-11T00:00:00Z",
				url: "https://github.com/acme/widgets/pull/42#issuecomment-101",
				isBot: false,
			},
		];
		const frame = await renderDetail({ comments });
		expect(frame).toContain("Conversation");
		expect(frame).toContain("2 comments");
		expect(frame).toContain("alice");
		expect(frame).toContain("Looks good overall");
		expect(frame).toContain("bob");
		expect(frame).toContain("Thanks for the review");
	});

	test("hides bot comments when showBotComments is false", async () => {
		const comments: IssueComment[] = [
			{
				id: 100,
				author: "alice",
				body: "Human comment",
				createdAt: "2026-03-10T00:00:00Z",
				url: "https://example.com",
				isBot: false,
			},
			{
				id: 101,
				author: "github-actions[bot]",
				body: "Bot comment",
				createdAt: "2026-03-10T00:00:00Z",
				url: "https://example.com",
				isBot: true,
			},
		];
		const frame = await renderDetail({ comments, showBotComments: false });
		expect(frame).toContain("1 comment");
		expect(frame).toContain("Human comment");
		expect(frame).not.toContain("Bot comment");
	});

	test("shows bot badge on bot comments", async () => {
		const comments: IssueComment[] = [
			{
				id: 100,
				author: "devin-ai[bot]",
				body: "Auto summary",
				createdAt: "2026-03-10T00:00:00Z",
				url: "https://example.com",
				isBot: true,
			},
		];
		const frame = await renderDetail({ comments, showBotComments: true });
		expect(frame).toContain("[bot]");
		expect(frame).toContain("devin-ai[bot]");
	});

	test("does not render conversation section when no comments exist", async () => {
		const frame = await renderDetail({ comments: [] });
		expect(frame).not.toContain("Conversation");
	});

	test("singular 'comment' label for 1 comment", async () => {
		const comments: IssueComment[] = [
			{
				id: 100,
				author: "alice",
				body: "Single comment",
				createdAt: "2026-03-10T00:00:00Z",
				url: "https://example.com",
				isBot: false,
			},
		];
		const frame = await renderDetail({ comments });
		expect(frame).toContain("1 comment");
		expect(frame).not.toContain("1 comments");
	});

	// ── Keybindings ────────────────────────────────────────────────────────

	test("Escape calls onExit", async () => {
		let exited = false;
		const { renderOnce, mockInput } = await testRender(
			() => (
				<DetailView
					pr={makeDetail()}
					threads={[]}
					comments={[]}
					loading={false}
					showResolved={false}
					showBotComments={true}
					onExit={() => {
						exited = true;
					}}
				/>
			),
			{ width: 80, height: 20 },
		);
		await renderOnce();
		mockInput.pressEscape();
		await new Promise((r) => setTimeout(r, 20));
		await renderOnce();
		expect(exited).toBe(true);
	});

	test("t calls onToggleResolved", async () => {
		let toggled = false;
		const { renderOnce, mockInput } = await testRender(
			() => (
				<DetailView
					pr={makeDetail()}
					threads={[]}
					comments={[]}
					loading={false}
					showResolved={false}
					showBotComments={true}
					onToggleResolved={() => {
						toggled = true;
					}}
				/>
			),
			{ width: 80, height: 20 },
		);
		await renderOnce();
		mockInput.pressKey("t");
		await renderOnce();
		expect(toggled).toBe(true);
	});

	test("b calls onToggleBotComments", async () => {
		let toggled = false;
		const { renderOnce, mockInput } = await testRender(
			() => (
				<DetailView
					pr={makeDetail()}
					threads={[]}
					comments={[]}
					loading={false}
					showResolved={false}
					showBotComments={true}
					onToggleBotComments={() => {
						toggled = true;
					}}
				/>
			),
			{ width: 80, height: 20 },
		);
		await renderOnce();
		mockInput.pressKey("b");
		await renderOnce();
		expect(toggled).toBe(true);
	});

	test("o calls onOpenInBrowser", async () => {
		let opened = false;
		const { renderOnce, mockInput } = await testRender(
			() => (
				<DetailView
					pr={makeDetail()}
					threads={[]}
					comments={[]}
					loading={false}
					showResolved={false}
					showBotComments={true}
					onOpenInBrowser={() => {
						opened = true;
					}}
				/>
			),
			{ width: 80, height: 20 },
		);
		await renderOnce();
		mockInput.pressKey("o");
		await renderOnce();
		expect(opened).toBe(true);
	});

	test("r calls onRefresh", async () => {
		let refreshed = false;
		const { renderOnce, mockInput } = await testRender(
			() => (
				<DetailView
					pr={makeDetail()}
					threads={[]}
					comments={[]}
					loading={false}
					showResolved={false}
					showBotComments={true}
					onRefresh={() => {
						refreshed = true;
					}}
				/>
			),
			{ width: 80, height: 20 },
		);
		await renderOnce();
		mockInput.pressKey("r");
		await renderOnce();
		expect(refreshed).toBe(true);
	});

	test("shows status bar with keybinding hints", async () => {
		const frame = await renderDetail();
		expect(frame).toContain("Esc close");
		expect(frame).toContain("o open");
		expect(frame).toContain("r refresh");
		expect(frame).toContain("show resolved");
	});

	test("status bar shows 'hide resolved' when showResolved is true", async () => {
		const frame = await renderDetail({ showResolved: true });
		expect(frame).toContain("hide resolved");
	});

	test("status bar shows 'hide bots' when showBotComments is true", async () => {
		const frame = await renderDetail({ showBotComments: true });
		expect(frame).toContain("hide bots");
	});

	test("status bar shows 'show bots' when showBotComments is false", async () => {
		const frame = await renderDetail({ showBotComments: false });
		expect(frame).toContain("show bots");
	});
});
