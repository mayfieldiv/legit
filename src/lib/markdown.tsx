/**
 * Markdown-to-opentui renderer.
 *
 * Parses markdown source via mdast-util-from-markdown and renders the AST
 * as SolidJS/opentui components. Block-level nodes (headings, paragraphs,
 * code blocks, lists, blockquotes, thematic breaks) are handled here.
 * Inline nodes (emphasis, strong, inlineCode, link) are rendered in a
 * separate inline pass.
 */

import { For, Switch, Match } from "solid-js";
import { fromMarkdown } from "mdast-util-from-markdown";
import type {
	Nodes,
	Heading,
	Paragraph,
	Code,
	List,
	ListItem,
	Blockquote,
	PhrasingContent,
} from "mdast";

// ── Public API ──────────────────────────────────────────────────────────────

export interface MarkdownBodyProps {
	source: string;
}

export function MarkdownBody(props: MarkdownBodyProps) {
	const tree = () => fromMarkdown(props.source);

	return (
		<box flexDirection="column" width="100%">
			<For each={tree().children}>{(node) => <MdBlock node={node} depth={0} />}</For>
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
		</Switch>
	);
}

// ── Individual block components ─────────────────────────────────────────────

function MdHeading(props: { node: Heading }) {
	const prefix = () => "#".repeat(props.node.depth) + " ";
	return (
		<box width="100%">
			<text>
				<span style={{ bold: true, fg: "cyan" }}>
					{prefix()}
					{collectInlineText(props.node.children)}
				</span>
			</text>
		</box>
	);
}

function MdParagraph(props: { node: Paragraph }) {
	return (
		<box width="100%">
			<text>{collectInlineText(props.node.children)}</text>
		</box>
	);
}

function MdCode(props: { node: Code }) {
	const lang = () => props.node.lang ?? "";
	return (
		<box flexDirection="column" width="100%" paddingLeft={2}>
			{lang() ? (
				<text>
					<span style={{ fg: "gray" }}>```{lang()}</span>
				</text>
			) : null}
			<text>
				<span style={{ fg: "yellow" }}>{props.node.value}</span>
			</text>
			{lang() ? (
				<text>
					<span style={{ fg: "gray" }}>```</span>
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
									{collectInlineText((child as Paragraph).children)}
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
									<span style={{ fg: "gray" }}>
										│ {collectInlineText((child as Paragraph).children)}
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
				<span style={{ fg: "gray" }}>────────────────────────────────────────</span>
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

// ── Inline text extraction ──────────────────────────────────────────────────
//
// For commit 5 (block nodes only), inline content is flattened to plain text.
// Commit 6 will replace this with styled inline rendering.

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
