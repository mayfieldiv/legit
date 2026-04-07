#!/usr/bin/env bun
/**
 * Generates a PR review tracker markdown file for all open PRs.
 *
 * Usage:
 *   bun docs/generate-pr-tracker.ts > docs/pr-review-tracker.md
 */

import { execFileSync, execSync } from "child_process";

const REPO = "immense/immybot";
const [owner, repo] = REPO.split("/");
const today = new Date();

// Resolve GH_TOKEN from macOS keychain if not already set.
// gh stores tokens as `go-keyring-base64:<base64>` in the keychain.
//
// NOTE: We must pass the token via the `env` option on every execSync call
// because Bun (unlike Node.js) does not propagate process.env mutations to
// child processes.  See https://github.com/oven-sh/bun/issues/
let ghToken = process.env.GH_TOKEN;
if (!ghToken) {
  try {
    const raw = execSync('security find-generic-password -s "gh:github.com" -a "" -w', {
      encoding: "utf-8",
    }).trim();
    const prefix = "go-keyring-base64:";
    if (raw.startsWith(prefix)) {
      ghToken = Buffer.from(raw.slice(prefix.length), "base64").toString("utf-8");
    } else {
      ghToken = raw;
    }
  } catch {
    // Fall through — gh will use its own auth
  }
}

const childEnv = ghToken ? { ...process.env, GH_TOKEN: ghToken } : process.env;

function gh(...args: string[]): string {
  return execFileSync("gh", args, {
    encoding: "utf-8",
    maxBuffer: 50 * 1024 * 1024,
    env: childEnv,
  }).trim();
}

// ── Fetch all open PRs ──────────────────────────────────────────────────────
const allPrs: Array<{
  number: number;
  title: string;
  author: { login: string; name: string; is_bot: boolean };
  createdAt: string;
  additions: number;
  deletions: number;
  reviewDecision: string;
  isDraft: boolean;
}> = JSON.parse(
  gh(
    "pr",
    "list",
    "--repo",
    REPO,
    "--state",
    "open",
    "--limit",
    "200",
    "--json",
    "number,title,author,createdAt,additions,deletions,reviewDecision,isDraft",
  ),
);

console.error(`Fetched ${allPrs.length} open PRs`);

// ── Fetch last commit date + mergeable status via GraphQL (batched) ─────────
interface PrMeta {
  lastCommitDate: string;
  mergeable: string;
}

const meta = new Map<number, PrMeta>();

const BATCH_SIZE = 50;
const numbers = allPrs.map((p) => p.number);

for (let i = 0; i < numbers.length; i += BATCH_SIZE) {
  const batch = numbers.slice(i, i + BATCH_SIZE);
  const aliases = batch
    .map(
      (n, idx) =>
        `pr${idx}: pullRequest(number: ${n}) { number mergeable commits(last: 1) { nodes { commit { committedDate } } } }`,
    )
    .join(" ");

  const query = `query { repository(owner: "${owner}", name: "${repo}") { ${aliases} } }`;

  const result = JSON.parse(gh("api", "graphql", "-f", `query=${query}`));

  const repoData = result.data.repository;
  for (let idx = 0; idx < batch.length; idx++) {
    const pr = repoData[`pr${idx}`];
    if (pr) {
      meta.set(pr.number, {
        lastCommitDate: pr.commits.nodes[0]?.commit?.committedDate ?? "unknown",
        mergeable: pr.mergeable ?? "UNKNOWN",
      });
    }
  }

  console.error(
    `  Fetched metadata batch ${Math.floor(i / BATCH_SIZE) + 1}/${Math.ceil(numbers.length / BATCH_SIZE)}`,
  );
}

// ── Helpers ─────────────────────────────────────────────────────────────────
function daysAgo(dateStr: string): number {
  return Math.floor((today.getTime() - new Date(dateStr).getTime()) / (1000 * 60 * 60 * 24));
}

function age(dateStr: string): string {
  const days = daysAgo(dateStr);
  if (days === 0) return "today";
  if (days === 1) return "1d";
  if (days < 30) return `${days}d`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;
  const years = Math.floor(months / 12);
  const rem = months % 12;
  return rem > 0 ? `${years}y${rem}mo` : `${years}y`;
}

function _fmtDate(dateStr: string): string {
  if (dateStr === "unknown") return "unknown";
  return new Date(dateStr).toISOString().slice(0, 10);
}

const prByNumber = new Map(allPrs.map((p) => [p.number, p]));

function row(num: number): string {
  const pr = prByNumber.get(num)!;
  const m = meta.get(num);
  const author = pr.author.name || pr.author.login;
  const commitAge = m && m.lastCommitDate !== "unknown" ? age(m.lastCommitDate) : "?";
  const conflict = m?.mergeable === "CONFLICTING" ? " **!!**" : "";
  const size = `+${pr.additions}/-${pr.deletions}`;
  const title = pr.title.length > 100 ? pr.title.slice(0, 97) + "..." : pr.title;
  const links = `[gh](https://github.com/${REPO}/pull/${num}) [dv](https://app.devin.ai/review/${REPO}/pull/${num})`;

  return `| ${conflict} | [#${num}](https://github.com/${REPO}/pull/${num}) | ${title} | ${author} | ${size} | ${age(pr.createdAt)} | ${commitAge} | ${links} |`;
}

// ── Configuration ───────────────────────────────────────────────────────────
const BOT_LOGINS = new Set(["app/devin-ai-integration", "app/copilot-swe-agent"]);
const _DRAFT_OWNERS: Record<string, string> = {
  dkattan: "Darren",
  colinblaise: "Colin",
  mayfieldiv: "Mayfield",
};

// ── Categorize ──────────────────────────────────────────────────────────────
const approved: number[] = [];
const needsReviewSmall: number[] = [];
const needsReviewMedium: number[] = [];
const needsReviewLarge: number[] = [];
const changesRequested: number[] = [];
const draftDarren: number[] = [];
const draftColin: number[] = [];
const draftMayfield: number[] = [];
const draftBot: number[] = [];
const draftOthers: number[] = [];
const stale: number[] = [];

for (const pr of allPrs) {
  const num = pr.number;
  const lines = pr.additions + pr.deletions;
  const login = pr.author.login;
  const isBot = pr.author.is_bot || BOT_LOGINS.has(login);

  if (pr.reviewDecision === "APPROVED") {
    approved.push(num);
  } else if (pr.reviewDecision === "CHANGES_REQUESTED") {
    changesRequested.push(num);
  } else if (pr.isDraft) {
    if (isBot) draftBot.push(num);
    else if (login === "dkattan") draftDarren.push(num);
    else if (login === "colinblaise") draftColin.push(num);
    else if (login === "mayfieldiv") draftMayfield.push(num);
    else draftOthers.push(num);
  } else {
    if (lines < 200) needsReviewSmall.push(num);
    else if (lines < 1000) needsReviewMedium.push(num);
    else needsReviewLarge.push(num);

    if (daysAgo(pr.createdAt) > 180) stale.push(num);
  }
}

const desc = (a: number, b: number) => b - a;
[
  approved,
  needsReviewSmall,
  needsReviewMedium,
  needsReviewLarge,
  changesRequested,
  draftDarren,
  draftColin,
  draftMayfield,
  draftBot,
  draftOthers,
  stale,
].forEach((arr) => arr.sort(desc));

const totalDrafts =
  draftDarren.length +
  draftColin.length +
  draftMayfield.length +
  draftBot.length +
  draftOthers.length;
const totalReview = needsReviewSmall.length + needsReviewMedium.length + needsReviewLarge.length;
const conflictCount = [...meta.values()].filter((m) => m.mergeable === "CONFLICTING").length;

const todayStr = today.toISOString().slice(0, 10);

// ── Render ──────────────────────────────────────────────────────────────────
const TABLE_HEADER = `| | PR | Title | Author | Size | Age | Last Commit | Links |
|---|---|---|---|---|---|---|---|`;

function section(title: string, items: number[]): string {
  if (items.length === 0) return "";
  return `## ${title} (${items.length})\n\n${TABLE_HEADER}\n${items.map(row).join("\n")}\n`;
}

const md = `# PR Review Tracker (${todayStr})

**${allPrs.length} open PRs** — ${approved.length} approved, ${totalReview} non-draft needing review, ${changesRequested.length} changes-requested, ${totalDrafts} drafts
**${conflictCount} PRs have merge conflicts** (marked with **!!**)

---

${section("Approved - Ready to Merge", approved)}
${section("Needs Review - Small (< 200 lines)", needsReviewSmall)}
${section("Needs Review - Medium (200-1000 lines)", needsReviewMedium)}
${section("Needs Review - Large (1000+ lines)", needsReviewLarge)}
${section("Changes Requested — authors need to address", changesRequested)}
${section("Draft PRs — Darren", draftDarren)}
${section("Draft PRs — Colin", draftColin)}
${section("Draft PRs — Others", draftOthers)}
${section("Draft PRs — Mayfield", draftMayfield)}
${section("Draft PRs — Bot", draftBot)}
${section("Stale Non-Draft PRs (created > 6 months ago)", stale)}`;

console.log(md);
