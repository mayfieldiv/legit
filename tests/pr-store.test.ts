import { describe, test, expect, afterAll } from "bun:test";
import { createRoot, createEffect } from "solid-js";
import { createPRStore, type ViewTarget } from "../src/lib/pr-store";
import { makePR } from "./helpers";
import {
	cleanupTmpDirs,
	createMockFetch,
	createTestLegit,
	makeSampleRestPR,
	makeGraphQLResponse,
	SAMPLE_GQL_META,
	SAMPLE_REST_PR,
	mockHttpFetch,
} from "./helpers";

afterAll(cleanupTmpDirs);

const jsonResponse = (body: unknown) =>
	new Response(JSON.stringify(body), {
		status: 200,
		headers: { "Content-Type": "application/json" },
	});

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

	test("enterDetail sets view to detail and exitDetail returns to list", async () => {
		const app = createTestLegit({
			httpFetch: mockHttpFetch([makeSampleRestPR(42)]),
		});
		const pr = makePR({ number: 42 });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });

				try {
					expect(store.view()).toEqual({ view: "list" });

					store.enterDetail(pr);
					const detailView = store.view() as ViewTarget & { view: "detail" };
					expect(detailView.view).toBe("detail");
					expect(detailView.pr.number).toBe(42);

					store.exitDetail();
					expect(store.view()).toEqual({ view: "list" });

					dispose();
					resolve();
				} catch (e) {
					dispose();
					reject(e);
				}
			});
		});
	});

	test("enterDetail fetches PR detail, threads, and issue comments", async () => {
		const emptyThreadsGql = {
			data: {
				repository: {
					pullRequest: {
						reviewThreads: {
							pageInfo: { hasNextPage: false, endCursor: null },
							nodes: [],
						},
					},
				},
			},
		};
		const fullThreadsGql = {
			data: {
				repository: {
					pullRequest: {
						reviewThreads: {
							pageInfo: { hasNextPage: false, endCursor: null },
							nodes: [
								{
									id: "RT_1",
									isResolved: false,
									path: "src/foo.ts",
									line: 10,
									comments: {
										nodes: [
											{
												id: "RC_1",
												author: { login: "bob", __typename: "User" },
												body: "Fix this",
												createdAt: "2026-03-10T00:00:00Z",
												url: "https://github.com/acme/widgets/pull/42#discussion_r1",
											},
										],
									},
								},
							],
						},
					},
				},
			},
		};
		const fetch = async (url: string | URL | Request, init?: RequestInit) => {
			const u = typeof url === "string" ? url : url.toString();
			if (init?.method === "POST" && u.includes("/graphql")) {
				const q: string = JSON.parse(String(init?.body ?? "{}")).query ?? "";
				if (q.includes("path") && q.includes("line")) return jsonResponse(fullThreadsGql);
				if (q.includes("reviewDecision"))
					return jsonResponse(makeGraphQLResponse([{ ...SAMPLE_GQL_META, number: 42 }]));
				return jsonResponse(emptyThreadsGql);
			}
			if (u.includes("/pulls?")) return jsonResponse([makeSampleRestPR(42)]);
			if (u.endsWith("/pulls/42"))
				return jsonResponse({ ...SAMPLE_REST_PR, number: 42, body: "PR description" });
			if (u.includes("/issues/42/comments"))
				return jsonResponse([
					{
						id: 200,
						user: { login: "alice", type: "User" },
						body: "Looks good",
						created_at: "2026-03-11T00:00:00Z",
						html_url: "https://github.com/acme/widgets/pull/42#issuecomment-200",
					},
				]);
			if (u.includes("/check-runs")) return jsonResponse({ check_runs: [] });
			if (u.includes("/reviews")) return jsonResponse([]);
			if (u.includes("/files")) return jsonResponse([]);
			return new Response("Not Found", { status: 404 });
		};
		const app = createTestLegit({ httpFetch: fetch as any });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				let enteredDetail = false;

				createEffect(() => {
					if (!store.loading() && !enteredDetail) {
						enteredDetail = true;
						const pr = store.prs()[0];
						if (pr) store.enterDetail(pr);
					}
					if (enteredDetail && !store.detailLoading() && store.detailPr()) {
						try {
							expect(store.detailPr()!.body).toBe("PR description");
							expect(store.detailThreads()).toHaveLength(1);
							expect(store.detailThreads()[0]!.id).toBe("RT_1");
							expect(store.detailComments()).toHaveLength(1);
							expect(store.detailComments()[0]!.author).toBe("alice");
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

	test("toggleResolved flips showResolved", async () => {
		const app = createTestLegit({ httpFetch: mockHttpFetch([makeSampleRestPR(42)]) });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				try {
					expect(store.showResolved()).toBe(false);
					store.toggleResolved();
					expect(store.showResolved()).toBe(true);
					store.toggleResolved();
					expect(store.showResolved()).toBe(false);
					dispose();
					resolve();
				} catch (e) {
					dispose();
					reject(e);
				}
			});
		});
	});

	test("toggleBotComments flips showBotComments (default true)", async () => {
		const app = createTestLegit({ httpFetch: mockHttpFetch([makeSampleRestPR(42)]) });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				try {
					expect(store.showBotComments()).toBe(true);
					store.toggleBotComments();
					expect(store.showBotComments()).toBe(false);
					store.toggleBotComments();
					expect(store.showBotComments()).toBe(true);
					dispose();
					resolve();
				} catch (e) {
					dispose();
					reject(e);
				}
			});
		});
	});

	test("exitDetail resets showResolved and showBotComments", async () => {
		const app = createTestLegit({ httpFetch: mockHttpFetch([makeSampleRestPR(42)]) });
		const pr = makePR({ number: 42 });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });
				try {
					store.enterDetail(pr);
					store.toggleResolved();
					store.toggleBotComments();
					expect(store.showResolved()).toBe(true);
					expect(store.showBotComments()).toBe(false);

					store.exitDetail();
					expect(store.showResolved()).toBe(false);
					expect(store.showBotComments()).toBe(true);

					dispose();
					resolve();
				} catch (e) {
					dispose();
					reject(e);
				}
			});
		});
	});

	test("exitDetail clears detail state", async () => {
		const app = createTestLegit({
			httpFetch: mockHttpFetch([makeSampleRestPR(42)]),
		});
		const pr = makePR({ number: 42 });

		await new Promise<void>((resolve, reject) => {
			createRoot((dispose) => {
				const store = createPRStore(app, { summaryDebounceMs: 0 });

				try {
					store.enterDetail(pr);
					expect(store.detailLoading()).toBe(true);

					store.exitDetail();
					expect(store.view()).toEqual({ view: "list" });
					expect(store.detailPr()).toBeUndefined();
					expect(store.detailThreads()).toEqual([]);
					expect(store.detailComments()).toEqual([]);
					expect(store.detailLoading()).toBe(false);

					dispose();
					resolve();
				} catch (e) {
					dispose();
					reject(e);
				}
			});
		});
	});
});
