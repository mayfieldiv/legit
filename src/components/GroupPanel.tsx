import { For } from "solid-js";
import type { GroupByKey } from "../lib/group-filter-engine";
import { theme } from "../lib/theme";

export interface GroupOption {
	key: GroupByKey;
	label: string;
}

export const GROUP_BY_OPTIONS: GroupOption[] = [
	{ key: "smart-status", label: "Smart status" },
	{ key: "author", label: "Author" },
	{ key: "repo", label: "Repo" },
	{ key: "size-category", label: "Size" },
	{ key: "label", label: "Label" },
	{ key: "none", label: "None (flat list)" },
];

interface GroupPanelProps {
	currentGroupBy: GroupByKey;
	selectedIndex: number;
}

export function GroupPanel(props: GroupPanelProps) {
	return (
		<box flexDirection="column" flexGrow={1} width="100%">
			<box height={1}>
				<text>
					<span style={{ fg: theme.accent, bold: true }}>Group by</span>
					<span style={{ fg: theme.muted }}>
						{" "}
						(j/k navigate · Enter select · Esc cancel)
					</span>
				</text>
			</box>
			<For each={GROUP_BY_OPTIONS}>
				{(opt, i) => {
					const isSelected = () => i() === props.selectedIndex;
					const isCurrent = () => opt.key === props.currentGroupBy;
					return (
						<box
							height={1}
							width="100%"
							backgroundColor={isSelected() ? theme.selectedBg : undefined}
						>
							<text>
								<span
									style={{
										fg: isSelected()
											? theme.selectedFg
											: isCurrent()
												? theme.success
												: undefined,
									}}
								>
									{isCurrent() ? "◉" : "○"} {opt.label}
								</span>
							</text>
						</box>
					);
				}}
			</For>
		</box>
	);
}
