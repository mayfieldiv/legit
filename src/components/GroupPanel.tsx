import { For } from "solid-js";
import type { GroupByKey } from "../lib/group-filter-engine";

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
					<span style={{ fg: "cyan", bold: true }}>Group by</span>
					<span style={{ fg: "gray" }}> (j/k navigate · Enter select · Esc cancel)</span>
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
							backgroundColor={isSelected() ? "blue" : undefined}
						>
							<text>
								<span
									style={{
										fg: isSelected()
											? "white"
											: isCurrent()
												? "green"
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
