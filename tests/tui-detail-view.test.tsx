import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { DetailView } from "../src/components/DetailView";
import type { PRDetail, CheckRun } from "../src/lib/types";
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
});
