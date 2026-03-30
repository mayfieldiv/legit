import { describe, test, expect, afterAll } from "bun:test";
import { createRoot, createEffect } from "solid-js";
import { createPRStore } from "../src/lib/pr-store";
import {
	cleanupTmpDirs,
	createMockFetch,
	createTestLegit,
	makeSampleRestPR,
	mockHttpFetch,
} from "./helpers";

afterAll(cleanupTmpDirs);

describe("createPRStore", () => {
	test("initial load settles with PRs and every pr has repoSlug", async () => {
		const app = createTestLegit({
			httpFetch: mockHttpFetch([makeSampleRestPR(42)]),
		});

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				createEffect(() => {
					if (!store.loading()) {
						try {
							const list = store.prs();
							expect(list.length).toBeGreaterThan(0);
							for (const pr of list) {
								expect(pr.repoSlug).toBeDefined();
								expect(pr.repoSlug).toBe(app.repoSlug);
							}
							dispose();
							resolve();
						} catch (e) {
							dispose();
							reject(e);
						}
					}
				});
			});
		});
	});

	test("tabs include All and tracked repos", async () => {
		const app = createTestLegit({ httpFetch: mockHttpFetch([makeSampleRestPR(1)]) });
		app.config.repos = ["acme/other"];

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				createEffect(() => {
					if (!store.loading()) {
						try {
							const tabs = store.tabs();
							expect(tabs[0]).toBe("All");
							expect(tabs).toContain(app.repoSlug);
							expect(tabs).toContain("acme/other");
							dispose();
							resolve();
						} catch (e) {
							dispose();
							reject(e);
						}
					}
				});
			});
		});
	});

	test("changeTab switches visible PRs and selection", async () => {
		const { fetch } = createMockFetch([
			{
				url: /\/repos\/acme\/widgets\/pulls\?/,
				response: { status: 200, body: [makeSampleRestPR(1)] },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: {
						data: {
							repository: {
								pr0: {
									number: 1,
									additions: 1,
									deletions: 1,
									reviewDecision: "APPROVED",
									mergeable: "MERGEABLE",
									commits: {
										nodes: [
											{
												commit: {
													committedDate: "2026-03-14T00:00:00Z",
													oid: "abc",
												},
											},
										],
									},
								},
							},
						},
					},
				},
			},
			{
				url: /\/repos\/acme\/gadgets\/pulls\?/,
				response: { status: 200, body: [makeSampleRestPR(2)] },
			},
			{
				url: /\/graphql/,
				method: "POST",
				response: {
					status: 200,
					body: {
						data: {
							repository: {
								pr0: {
									number: 2,
									additions: 1,
									deletions: 1,
									reviewDecision: "APPROVED",
									mergeable: "MERGEABLE",
									commits: {
										nodes: [
											{
												commit: {
													committedDate: "2026-03-14T00:00:00Z",
													oid: "abc",
												},
											},
										],
									},
								},
							},
						},
					},
				},
			},
		]);
		const app = createTestLegit({ httpFetch: fetch });
		app.config.repos = ["acme/widgets", "acme/gadgets"];

		let store: ReturnType<typeof createPRStore>;
		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				store = createPRStore(app, { summaryDebounceMs: 0 });
				createEffect(() => {
					if (!store!.loading() && store!.prs().length >= 2) {
						try {
							store!.changeTab(2);
							const visible = store!.prs();
							expect(visible.every((p) => p.repoSlug === "acme/gadgets")).toBe(true);
							expect(store!.selectedPr()?.number).toBe(2);
							dispose();
							resolve();
						} catch (e) {
							dispose();
							reject(e);
						}
					}
				});
			});
		});
	});

	test("refreshAll keeps the current tab (repo tab stays selected)", async () => {
		const app = createTestLegit({
			httpFetch: mockHttpFetch([makeSampleRestPR(7)]),
		});

		let store: ReturnType<typeof createPRStore>;
		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				store = createPRStore(app, { summaryDebounceMs: 0 });
				let sawLoaded = false;
				createEffect(() => {
					try {
						if (!store!.loading() && !sawLoaded) {
							sawLoaded = true;
							store!.changeTab(1);
							expect(store!.activeTab()).toBe(1);
							store!.refreshAllActive();
						}
						if (sawLoaded && store!.loading()) {
							expect(store!.activeTab()).toBe(1);
						}
						if (sawLoaded && !store!.loading()) {
							expect(store!.activeTab()).toBe(1);
							dispose();
							resolve();
						}
					} catch (e) {
						dispose();
						reject(e);
					}
				});
			});
		});
	});

	test("showRepo is false with a single tracked repo (two tabs: All + repo)", async () => {
		const app = createTestLegit({ httpFetch: mockHttpFetch([makeSampleRestPR(1)]) });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				createEffect(() => {
					if (!store.loading()) {
						try {
							expect(store.tabs().length).toBe(2);
							expect(store.showRepo()).toBe(false);
							dispose();
							resolve();
						} catch (e) {
							dispose();
							reject(e);
						}
					}
				});
			});
		});
	});

	test("fetch failure sets error and loading finishes", async () => {
		const { fetch } = createMockFetch([
			{ url: /pulls/, response: { status: 500, body: { message: "oops" } } },
		]);
		const app = createTestLegit({ httpFetch: fetch });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				createEffect(() => {
					if (!store.loading()) {
						try {
							expect(store.error()).toContain("500");
							dispose();
							resolve();
						} catch (e) {
							dispose();
							reject(e);
						}
					}
				});
			});
		});
	});
});
