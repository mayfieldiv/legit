export default {
	"*.{ts,tsx,js,jsx,mts,mjs,cjs}": ["oxfmt --write", "oxlint", () => "tsgo --noEmit"],
	"*.{json,jsonc,css,md}": "oxfmt --write",
};
