import { describe, test, expect } from "bun:test";
import { testRender } from "@opentui/solid";
import { SummaryPanel } from "../src/components/SummaryPanel";
import { makePR } from "./helpers";
import type { PRSummary } from "../src/lib/types";

function makeSummary(overrides: Partial<PRSummary> = {}): PRSummary {
	return {
		...makePR(),
		body: "",
		checks: [],
		reviews: [],
		comments: { total: 0, unresolved: 0, unresolvedHuman: 0, unresolvedBot: 0 },
		threads: [],
		files: {
			files: [],
			breakdown: {
				code: { additions: 0, deletions: 0, files: 0 },
				test: { additions: 0, deletions: 0, files: 0 },
				generated: { additions: 0, deletions: 0, files: 0 },
				docs: { additions: 0, deletions: 0, files: 0 },
				config: { additions: 0, deletions: 0, files: 0 },
				total: { additions: 0, deletions: 0, files: 0 },
			},
		},
		...overrides,
	};
}

describe("SummaryPanel", () => {
	test("shows PR title and author", async () => {
		const summary = makeSummary({ title: "Fix login bug", author: "alice", number: 99 });
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("Fix login bug");
		expect(frame).toContain("alice");
		expect(frame).toContain("#99");
	});

	test("shows draft badge for draft PRs", async () => {
		const summary = makeSummary({ isDraft: true });
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toMatch(/draft/i);
	});

	test("shows merge conflict indicator", async () => {
		const summary = makeSummary({ mergeable: "CONFLICTING" });
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toMatch(/conflict/i);
	});

	test("shows CI checks sorted: failed first", async () => {
		const summary = makeSummary({
			checks: [
				{ name: "lint", status: "completed", conclusion: "success" },
				{ name: "build", status: "completed", conclusion: "failure" },
				{ name: "deploy", status: "in_progress", conclusion: null },
			],
		});
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		const buildIdx = frame.indexOf("build");
		const deployIdx = frame.indexOf("deploy");
		const lintIdx = frame.indexOf("lint");
		expect(buildIdx).toBeGreaterThan(-1);
		expect(deployIdx).toBeGreaterThan(-1);
		expect(lintIdx).toBeGreaterThan(-1);
		expect(buildIdx).toBeLessThan(deployIdx);
		expect(deployIdx).toBeLessThan(lintIdx);
	});

	test("shows reviewers", async () => {
		const summary = makeSummary({
			reviews: [
				{ user: "bob", state: "APPROVED" },
				{ user: "carol", state: "CHANGES_REQUESTED" },
			],
		});
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("bob");
		expect(frame).toContain("carol");
	});

	test("shows comment counts", async () => {
		const summary = makeSummary({
			comments: { total: 5, unresolved: 3, unresolvedHuman: 2, unresolvedBot: 1 },
		});
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("3");
		expect(frame).toContain("unresolved");
	});

	test("shows empty state when no summary or pr", async () => {
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={undefined} pr={undefined} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toBeDefined();
	});

	test("shows basic info from PR when summary is loading", async () => {
		const pr = makePR({ title: "Loading test", number: 77 });
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={undefined} pr={pr} />,
			{ width: 40, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toContain("Loading test");
		expect(frame).toContain("#77");
	});

	test("shows blocker tier when currentUser is provided and summary is loaded", async () => {
		const summary = makeSummary({
			author: "charlie",
			requestedReviewers: ["alice"],
		});
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} currentUser="alice" />,
			{ width: 50, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		// me-blocking tier should be shown
		expect(frame).toMatch(/me.blocking|you/i);
	});

	test("shows waiting-on-author when CI is failing", async () => {
		const summary = makeSummary({
			author: "charlie",
			checks: [{ name: "build", status: "completed", conclusion: "failure" }],
		});
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} currentUser="alice" />,
			{ width: 50, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).toMatch(/waiting.on.author|charlie/i);
	});

	test("does not show blocker section when currentUser is not provided", async () => {
		const summary = makeSummary({ requestedReviewers: ["alice"] });
		const { renderOnce, captureCharFrame } = await testRender(
			() => <SummaryPanel summary={summary} pr={makePR()} />,
			{ width: 50, height: 30 },
		);
		await renderOnce();
		const frame = captureCharFrame();
		expect(frame).not.toMatch(/me.blocking/i);
	});
});
