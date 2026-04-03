/**
 * Markdown-to-opentui renderer.
 *
 * Parses markdown source via mdast-util-from-markdown and renders the AST
 * as SolidJS/opentui components. Block-level nodes (headings, paragraphs,
 * code blocks, lists, blockquotes, thematic breaks) produce layout boxes.
 * Inline nodes (emphasis, strong, inlineCode, link) produce styled spans.
 */

import { For, Show, Switch, Match, createSignal } from "solid-js";
import { fromMarkdown } from "mdast-util-from-markdown";
import { fromHtml } from "hast-util-from-html";
import type { Element as HastElement } from "hast";
import { useDetails, type DetailsHandle } from "./details-store";
import type { MouseEvent as OtuiMouseEvent } from "@opentui/core";
import { theme } from "./theme";
import type {
	Nodes,
	Heading,
	Paragraph,
	Code,
	List,
	ListItem,
	Blockquote,
	PhrasingContent,
	Strong,
	Emphasis,
	InlineCode,
	Link,
	Image,
} from "mdast";

/** True when the raw HTML value contains only comment nodes (no elements/text). */
function isHtmlCommentOnly(value: string): boolean {
	const tree = fromHtml(value, { fragment: true });
	return tree.children.length > 0 && tree.children.every((n) => n.type === "comment");
}

// ── <details> grouping ──────────────────────────────────────────────────────

/** A grouped <details> block: summary text + inner mdast content nodes. */
export interface DetailsGroup {
	type: "detailsGroup";
	summary: string;
	children: Nodes[];
}

type BlockOrDetails = Nodes | DetailsGroup;

/** Check if an html node opens a <details> block. */
function isDetailsOpen(value: string): boolean {
	const tree = fromHtml(value, { fragment: true });
	return tree.children.some(
		(n) => n.type === "element" && (n as HastElement).tagName === "details",
	);
}

/** Check if an html node is a </details> closing tag. */
function isDetailsClose(value: string): boolean {
	return /^\s*<\/details>\s*$/.test(value);
}

/** Extract summary text from the opening <details> html block. */
function extractSummary(value: string): string {
	const tree = fromHtml(value, { fragment: true });
	for (const node of tree.children) {
		if (node.type === "element" && (node as HastElement).tagName === "details") {
			for (const child of (node as HastElement).children) {
				if (child.type === "element" && (child as HastElement).tagName === "summary") {
					return collectHastText(child as HastElement);
				}
			}
		}
	}
	return "Details";
}

/** Recursively extract plain text from a hast element. */
function collectHastText(node: HastElement): string {
	let result = "";
	for (const child of node.children) {
		if (child.type === "text") result += child.value;
		else if (child.type === "element") result += collectHastText(child as HastElement);
	}
	return result;
}

/**
 * Pre-process mdast children to group <details>…</details> into DetailsGroup nodes.
 * Markdown content between the opening and closing html tags becomes the group's children.
 */
function groupDetailsBlocks(nodes: Nodes[]): BlockOrDetails[] {
	const result: BlockOrDetails[] = [];
	let i = 0;
	while (i < nodes.length) {
		const node = nodes[i]!;
		if (node.type === "html" && isDetailsOpen((node as any).value ?? "")) {
			const summary = extractSummary((node as any).value ?? "");
			const content: Nodes[] = [];
			i++;
			while (i < nodes.length) {
				const n = nodes[i]!;
				if (n.type === "html" && isDetailsClose((n as any).value ?? "")) {
					i++;
					break;
				}
				content.push(n);
				i++;
			}
			result.push({ type: "detailsGroup", summary, children: content });
		} else {
			result.push(node);
			i++;
		}
	}
	return result;
}

// ── Public API ──────────────────────────────────────────────────────────────

export interface MarkdownBodyProps {
	source: string;
}

export function MarkdownBody(props: MarkdownBodyProps) {
	const tree = () => fromMarkdown(props.source);
	const grouped = () => groupDetailsBlocks(tree().children);

	return (
		<box flexDirection="column" width="100%">
			<For each={grouped()}>
				{(node) =>
					node.type === "detailsGroup" ? (
						<MdDetails group={node as DetailsGroup} />
					) : (
						<MdBlock node={node as Nodes} depth={0} />
					)
				}
			</For>
		</box>
	);
}

// ── <details> component ─────────────────────────────────────────────────────

function MdDetails(props: { group: DetailsGroup }) {
	const ctrl = useDetails();
	// If inside a DetailsCtx, register; otherwise use local state.
	const handle: DetailsHandle = ctrl
		? ctrl.register()
		: (() => {
				const [expanded, setExpanded] = createSignal(false);
				return { expanded, toggle: () => setExpanded(!expanded()) };
			})();

	return (
		<box flexDirection="column" width="100%">
			<box
				width="100%"
				height={1}
				onMouseDown={(e: OtuiMouseEvent) => {
					e.preventDefault();
					handle.toggle();
				}}
			>
				<text>
					<span style={{ fg: theme.accent }}>{handle.expanded() ? "▼ " : "▶ "}</span>
					<span style={{ bold: true }}>{props.group.summary}</span>
				</text>
			</box>
			<Show when={handle.expanded()}>
				<box flexDirection="column" width="100%" paddingLeft={2}>
					<For each={groupDetailsBlocks(props.group.children)}>
						{(node) =>
							node.type === "detailsGroup" ? (
								<MdDetails group={node as DetailsGroup} />
							) : (
								<MdBlock node={node as Nodes} depth={0} />
							)
						}
					</For>
				</box>
			</Show>
		</box>
	);
}

// ── Block-level renderer ────────────────────────────────────────────────────

interface MdBlockProps {
	node: Nodes;
	depth: number;
}

function MdBlock(props: MdBlockProps) {
	return (
		<Switch fallback={<FallbackBlock node={props.node} />}>
			<Match when={props.node.type === "heading"}>
				<MdHeading node={props.node as Heading} />
			</Match>
			<Match when={props.node.type === "paragraph"}>
				<MdParagraph node={props.node as Paragraph} />
			</Match>
			<Match when={props.node.type === "code"}>
				<MdCode node={props.node as Code} />
			</Match>
			<Match when={props.node.type === "list"}>
				<MdList node={props.node as List} depth={props.depth} />
			</Match>
			<Match when={props.node.type === "blockquote"}>
				<MdBlockquote node={props.node as Blockquote} depth={props.depth} />
			</Match>
			<Match when={props.node.type === "thematicBreak"}>
				<MdThematicBreak />
			</Match>
			<Match when={props.node.type === "html"}>
				{!isHtmlCommentOnly((props.node as any).value ?? "") && (
					<box width="100%">
						<text>
							<span style={{ fg: theme.muted }}>[html content]</span>
						</text>
					</box>
				)}
			</Match>
		</Switch>
	);
}

// ── Individual block components ─────────────────────────────────────────────

function MdHeading(props: { node: Heading }) {
	const prefix = () => "#".repeat(props.node.depth) + " ";
	return (
		<box width="100%">
			<text>
				<span style={{ bold: true, fg: theme.accent }}>
					{prefix()}
					<InlineNodes nodes={props.node.children} />
				</span>
			</text>
		</box>
	);
}

function MdParagraph(props: { node: Paragraph }) {
	return (
		<box width="100%">
			<text>
				<InlineNodes nodes={props.node.children} />
			</text>
		</box>
	);
}

function MdCode(props: { node: Code }) {
	const lang = () => props.node.lang ?? "";
	return (
		<box flexDirection="column" width="100%" paddingLeft={2}>
			{lang() ? (
				<text>
					<span style={{ fg: theme.muted }}>```{lang()}</span>
				</text>
			) : null}
			<text>
				<span style={{ fg: theme.code }}>{props.node.value}</span>
			</text>
			{lang() ? (
				<text>
					<span style={{ fg: theme.muted }}>```</span>
				</text>
			) : null}
		</box>
	);
}

function MdList(props: { node: List; depth: number }) {
	const ordered = () => props.node.ordered ?? false;
	return (
		<box flexDirection="column" width="100%">
			<For each={props.node.children}>
				{(item, index) => (
					<MdListItem
						node={item as ListItem}
						depth={props.depth}
						ordered={ordered()}
						index={index()}
					/>
				)}
			</For>
		</box>
	);
}

function MdListItem(props: { node: ListItem; depth: number; ordered: boolean; index: number }) {
	const indent = () => "  ".repeat(props.depth);
	const bullet = () => (props.ordered ? `${props.index + 1}. ` : "• ");

	return (
		<box flexDirection="column" width="100%">
			<For each={props.node.children}>
				{(child, childIdx) => {
					if (child.type === "paragraph") {
						return (
							<box width="100%">
								<text>
									{childIdx() === 0 ? indent() + bullet() : indent() + "  "}
									<InlineNodes nodes={(child as Paragraph).children} />
								</text>
							</box>
						);
					}
					// Nested list or other block
					return <MdBlock node={child} depth={props.depth + 1} />;
				}}
			</For>
		</box>
	);
}

function MdBlockquote(props: { node: Blockquote; depth: number }) {
	return (
		<box flexDirection="column" width="100%" paddingLeft={2}>
			<For each={props.node.children}>
				{(child) => {
					if (child.type === "paragraph") {
						return (
							<box width="100%">
								<text>
									<span style={{ fg: theme.muted }}>
										│ <InlineNodes nodes={(child as Paragraph).children} />
									</span>
								</text>
							</box>
						);
					}
					return <MdBlock node={child} depth={props.depth} />;
				}}
			</For>
		</box>
	);
}

function MdThematicBreak() {
	return (
		<box width="100%">
			<text>
				<span style={{ fg: theme.muted }}>────────────────────────────────────────</span>
			</text>
		</box>
	);
}

function FallbackBlock(props: { node: Nodes }) {
	// Render unknown block types as plain text if they have a value
	const value = "value" in props.node ? String(props.node.value) : "";
	if (!value) return null;
	return (
		<box width="100%">
			<text>{value}</text>
		</box>
	);
}

// ── Inline renderer ───────────────────────────────────────────────────────

/** Render an array of inline/phrasing nodes as styled <span> elements. */
export function InlineNodes(props: { nodes: PhrasingContent[] }) {
	return <For each={props.nodes}>{(node) => <InlineNode node={node} />}</For>;
}

function InlineNode(props: { node: PhrasingContent }) {
	return (
		<Switch fallback={<InlineFallback node={props.node} />}>
			<Match when={props.node.type === "text"}>
				<span>{(props.node as { type: "text"; value: string }).value}</span>
			</Match>
			<Match when={props.node.type === "strong"}>
				<span style={{ bold: true }}>
					<InlineNodes nodes={(props.node as Strong).children} />
				</span>
			</Match>
			<Match when={props.node.type === "emphasis"}>
				<span style={{ italic: true }}>
					<InlineNodes nodes={(props.node as Emphasis).children} />
				</span>
			</Match>
			<Match when={props.node.type === "inlineCode"}>
				<span style={{ fg: theme.code }}>{(props.node as InlineCode).value}</span>
			</Match>
			<Match when={props.node.type === "link"}>
				<a href={(props.node as Link).url}>
					<InlineNodes nodes={(props.node as Link).children} />
				</a>
			</Match>
			<Match when={props.node.type === "image"}>
				<span style={{ fg: theme.muted }}>
					[image: {(props.node as Image).alt ?? (props.node as Image).url}]
				</span>
			</Match>
		</Switch>
	);
}

function InlineFallback(props: { node: PhrasingContent }) {
	if ("value" in props.node) return <span>{String(props.node.value)}</span>;
	if ("children" in props.node)
		return <InlineNodes nodes={(props.node as { children: PhrasingContent[] }).children} />;
	return null;
}

// ── Plain text extraction (utility) ───────────────────────────────────────

/** Recursively extract plain text from inline/phrasing nodes. */
export function collectInlineText(nodes: PhrasingContent[]): string {
	let result = "";
	for (const node of nodes) {
		if (node.type === "text") {
			result += node.value;
		} else if (node.type === "inlineCode") {
			result += node.value;
		} else if ("children" in node) {
			result += collectInlineText(node.children as PhrasingContent[]);
		} else if ("value" in node) {
			result += String(node.value);
		}
	}
	return result;
}
