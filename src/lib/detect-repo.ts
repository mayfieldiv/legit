import { execFileSync } from "child_process";

export interface RepoInfo {
	owner: string;
	repo: string;
}

/**
 * Detect the GitHub owner/repo from the git remote origin URL.
 * Supports SSH (git@github.com:owner/repo.git) and HTTPS (https://github.com/owner/repo.git) formats.
 */
export function detectRepo(cwd?: string): RepoInfo {
	const dir = cwd ?? process.cwd();

	let remoteUrl: string;
	try {
		remoteUrl = execFileSync("git", ["remote", "get-url", "origin"], {
			cwd: dir,
			encoding: "utf-8",
			stdio: ["pipe", "pipe", "pipe"],
		}).trim();
	} catch {
		throw new Error(`No git remote 'origin' found in ${dir}`);
	}

	return parseRemoteUrl(remoteUrl);
}

export function parseRemoteUrl(url: string): RepoInfo {
	// SSH: git@github.com:owner/repo.git
	const sshMatch = url.match(/git@github\.com:([^/]+)\/([^/.]+)(?:\.git)?$/);
	if (sshMatch) {
		return { owner: sshMatch[1], repo: sshMatch[2] };
	}

	// HTTPS: https://github.com/owner/repo.git
	const httpsMatch = url.match(
		/https?:\/\/github\.com\/([^/]+)\/([^/.]+)(?:\.git)?$/,
	);
	if (httpsMatch) {
		return { owner: httpsMatch[1], repo: httpsMatch[2] };
	}

	throw new Error(`Cannot parse GitHub remote URL: ${url}`);
}
