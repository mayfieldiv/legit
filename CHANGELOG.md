# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.1.0] - 2026-04-07

### Fixed

- Merge conflicts not detected: GitHub computes mergeability lazily and returns UNKNOWN on the first query; legit now schedules a delayed retry after the PR list settles so conflicts resolve without blocking enrichment (#32)
- Detail view: pressing `o` on PR body opened the wrong repo URL; repo slug was missing from the fetched `PRDetail`, causing fallback to the CWD repo (#27)
- Detail view: repo slug and full PR URL not shown in header for the same reason (#27)

### Changed

- GraphQL review-status batches now fire concurrently instead of sequentially, cutting enrichment time from ~14s to ~2.5s for large repos (#32)

### Added

- Detail view shows merge status (conflict / mergeable / unknown) on the branch line (#32)

### Changed

- Move `ViewTarget` and view state from `AppShell` into `PRStore`; `ListView` uses `onEnterDetail` callback instead of `onNavigate` (#7)
- `PRStore.enterDetail` fetches PR detail, full review threads, and issue comments in parallel (#7)
- `PRStore.toggleResolved` and `PRStore.toggleBotComments` for detail view filtering (#7)
- Detail view: PR header, markdown-rendered description, and CI checks section (#7)
- Detail view: review threads with file path/line, resolved/unresolved status, and conversation (issue comments) sections; filtered by `showResolved` and `showBotComments` (#7)
- Detail view keybindings: Esc (exit), t (toggle resolved), b (toggle bots), o (open in browser), r (refresh); status bar with hints (#7)

### Added

- Domain types for PR detail view: `ReviewComment`, `FullReviewThread`, `IssueComment` (#7)
- Transport methods `fetchFullReviewThreads` and `listIssueComments` for full comment data (#7)
- Client methods `fetchFullReviewThreads` and `fetchIssueComments` with bot detection (#7)
- `legit comments <number>` CLI subcommand — outputs review threads and issue comments as JSON (#7)
- Markdown-to-TUI renderer for PR descriptions — headings, paragraphs, code blocks, lists, blockquotes, thematic breaks; styled inline rendering for bold, italic, inline code, links, and images (#7)
