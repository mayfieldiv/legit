import type { JSX } from "solid-js";

/**
 * A single-line status bar showing keyboard shortcut hints.
 *
 * Renders shared shortcuts (o GitHub, d Devin, r refresh) plus
 * any view-specific extras passed as children.
 */
export function StatusBar(props: { children?: JSX.Element }) {
	return (
		<box width="100%" height={1}>
			<text>
				<span style={{ fg: "gray" }}>
					j/k nav · ↵ open · o GitHub · d Devin · r refresh
					{props.children}
				</span>
			</text>
		</box>
	);
}
