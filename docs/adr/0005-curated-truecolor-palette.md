# Curated dark-first truecolor palette over terminal-adaptive ANSI colors

legit rendered entirely with ANSI-named colours (`Color::Cyan`, `Color::Yellow`, …), so it inherited the user's terminal theme automatically — a real virtue. To get GHUI-style subtle selection fills and real-colour label chips, neither of which is expressible in the 16-colour ANSI palette, we adopt a single curated, dark-tuned truecolor `Palette` of semantic roles and route every view call site through it. We accept the loss of automatic terminal adaptivity and are explicitly dark-first.

## Considered options

- **Hybrid — keep ANSI-named for most roles, truecolor only for chips and the selection fill.** Rejected: the one genuinely terminal-dependent role is the selection fill (a fixed dark slate looks broken on a light terminal), so a hybrid still takes the adaptivity hit on the role that matters, without the consistency payoff. Label chips are self-contained (they carry their own background plus a contrast-flipped foreground derived from the label's own colour), so they need no palette and work under any model — they are not what forces the decision.
- **Full theme system now — multiple themes plus a terminal-derived "system" theme (GHUI's `makeSystemColors`: luminance ramps, contrast derivation, persistence).** Deferred, not rejected: it is a much larger build. The semantic-palette seam introduced here lets it drop in later as an additive change without re-touching call sites.

## Consequences

- The ~68 `Color::` call sites across the four view modules (`view.rs`, `view/list.rs`, `view/summary.rs`, `view/detail.rs`) move behind named palette roles.
- Dark-first: light terminals will look wrong until a future theme / system-derive lands. The palette is the seam that makes that future work additive rather than a rewrite.
- The palette is hardcoded for now — no user-facing colour config. A `Palette` type with one default instance, threaded through the view layer.
