# Graphite — Design System

The visual identity of Graphite, derived from the material the product is named after: graphite the carbon allotrope. Dark, crystalline, layered, conductive. The graph edges are conductive paths through crystalline structure.

## Principle

**Show the mechanism.** Every visual choice exists to make the graph legible, the evidence credible, and the tool feel like an instrument. Premium through restraint and precision, not through decoration.

The landing page sells with narrative and evidence. The design system ensures the evidence looks like it came from a serious tool, not a template.

## Metaphor

Graphite (the mineral): layers of carbon atoms in hexagonal lattice, connected by conductive paths. The product is the same: layers of code connected by dependency edges. The visual language draws from:

- **Crystalline structure** — hexagonal/geometric patterns as subtle texture, not as decoration
- **Conductivity** — edges glow, paths light up, traversal is visible energy
- **Layers** — depth through stacked planes, not through shadows or blur
- **Dark material** — graphite is near-black with metallic sheen; dark-first

## Palette

Dark-first. Light mode exists but dark is primary and the landing page identity.

| Token | Dark | Light | Role |
|---|---|---|---|
| ground | `#0C0C0E` | `#F5F3F0` | page background, carbon surface |
| surface | `#161619` | `#FFFFFF` | cards, elevated panels |
| surface-alt | `#1E1E22` | `#F0EDEA` | code blocks, secondary panels |
| ink | `#E8E4DF` | `#1A1A1E` | primary text |
| secondary | `#8A8690` | `#6B6770` | metadata, captions |
| muted | `#4A464F` | `#C5C1C8` | borders, dividers, inactive |
| accent | `#00E5A0` | `#00B87D` | graph edges, primary action, traversal glow |
| accent-dim | `#00E5A0` at 20% | `#00B87D` at 15% | accent wash, hover states |
| confidence-extracted | `#00E5A0` | `#00B87D` | EXTRACTED edges — solid, trusted |
| confidence-inferred | `#FFB84D` | `#D4940A` | INFERRED edges — warm amber, partial trust |
| confidence-ambiguous | `#FF6B6B` | `#CC4444` | AMBIGUOUS edges — soft red, needs verification |
| warning | `#FFB84D` | `#D4940A` | staleness indicators, caution |
| error | `#FF6B6B` | `#CC4444` | parse failures, broken edges |

Contrast floor: 4.5:1 text on ground, 3:1 for decorative/structural elements.

The three confidence colors (green/amber/red) are semantic, not decorative. They map directly to the engine's edge confidence tiers and appear in CLI output, landing page evidence sections, and future UI.

## Typography

| Role | Family | Weight | Size | Notes |
|---|---|---|---|---|
| Display | Inter | 700 | 48–64px | Headlines, hero text |
| Heading | Inter | 600 | 24–36px | Section titles |
| Body | Inter | 400 | 16–18px | Paragraphs, explanations |
| Caption | Inter | 400 | 13–14px | Metadata, footnotes |
| Code | JetBrains Mono | 400 | 14–15px | All code, CLI output, graph output |
| Code emphasis | JetBrains Mono | 700 | 14–15px | Highlighted symbols, changed code |

Inter for its geometric clarity and wide weight range. JetBrains Mono for code because Graphite is a Rust tool and JetBrains Mono is native to the dev audience. No serif anywhere.

## Spacing

8px base unit. All spacing is multiples of 8.

| Token | Value | Use |
|---|---|---|
| xs | 4px | Tight inline spacing |
| sm | 8px | Between related elements |
| md | 16px | Between components |
| lg | 24px | Section padding |
| xl | 48px | Between major sections |
| 2xl | 96px | Landing page section gaps |

## Graph Visualization Tokens

The graph is the product. Its rendering has dedicated tokens.

| Element | Style | Notes |
|---|---|---|
| Node (symbol) | 6px circle, `ink` fill, 1px `muted` stroke | Quiet by default |
| Node (active) | 8px circle, `accent` fill, 2px `accent` stroke, glow | The symbol being queried |
| Node (in blast) | 6px circle, `accent-dim` fill | In the blast radius |
| Edge (EXTRACTED) | 1px solid `accent` | Trusted path |
| Edge (INFERRED) | 1px dashed `confidence-inferred` | Partial trust, visible but uncertain |
| Edge (AMBIGUOUS) | 1px dotted `confidence-ambiguous` | Needs verification |
| Edge (traversal) | 2px solid `accent`, animated pulse | Active traversal, energy flowing |
| Depth label | `caption` size, `secondary` color | Distance from queried symbol |
| Cluster boundary | 1px dashed `muted`, rounded | Community/module boundary |

Graph visualizations on the landing page show selective subgraphs (10–30 nodes), never whole-repo hairballs. The visualization demonstrates blast radius, not repo scale.

## Component Patterns

### Code Block

```
background: surface-alt
border: 1px solid muted
border-radius: 4px
padding: 16px
font: Code 14px
line-height: 1.6
```

Line numbers in `secondary`. Highlighted lines get `accent-dim` background. Syntax highlighting follows the palette (accent for keywords, ink for identifiers, secondary for comments, confidence-inferred for strings).

### CLI Output Block

Same as code block but with a `$ graphite` prompt header in `secondary` and output in structured format matching real CLI output. Graph annotations (blast radius markers, confidence tags) use their semantic colors inline.

### Evidence Card

For before/after comparisons on the landing page:

```
background: surface
border: 1px solid muted
border-radius: 8px
padding: 24px
```

Two columns: "Without Graphite" (neutral/muted) and "With Graphite" (accent highlights). Numbers (turns, seconds, files read) are display-size in their respective tones.

### Metric Tile

For hero stats (turns saved, speed improvement):

```
number: Display 48px, accent
label: Caption, secondary
background: transparent or surface with accent-dim border
```

## Motion

Minimal. One animation vocabulary:

- **Traversal pulse**: edge lights up from source to target, 400ms ease-out. Used for graph hero animation.
- **Fade in on scroll**: content sections fade up, 300ms ease-out, staggered 50ms. Standard scroll entrance.
- **Count up**: metric numbers animate from 0 to final value on viewport entry, 800ms ease-out.

No parallax. No continuous motion outside the graph hero. `prefers-reduced-motion` disables all animation.

## Landing Page Structure (narrative arc)

The landing page follows a storytelling structure grounded in evidence:

1. **Hero**: tagline + one-line pitch + animated graph showing blast radius traversal
2. **Pain**: "Your agent reads 30 files before it acts." Visualize the wasted exploration.
3. **Mechanism**: how Graphite works in three beats (extract, persist, serve). The architecture diagram, simplified.
4. **Evidence**: before/after from real bench pilots. Turns, wall-clock, transcript excerpts. This is the proof.
5. **CLI demo**: real commands, real output. The dev sees what they'd type.
6. **Confidence**: edge confidence visualization. The honesty differentiator.
7. **Architecture**: hybrid engine, always-hot daemon, one binary. For the performance-minded.
8. **Landscape**: how it compares. Measured, specific, from positioning.md.
9. **Getting started**: `cargo build --release`, three commands, done.

~50% of page is evidence (bench results, CLI output, graph visualizations). The rest is narrative framing.

## Dark/Light

Dark is primary. Landing page ships dark-only with system-aware override for the future UI. CSS custom properties on `:root` with `@media (prefers-color-scheme: dark)` guard.

## Anti-Patterns

- Whole-repo force-directed graph as hero. Past 50 nodes on a landing page it's noise.
- Neon/cyberpunk aesthetic. Graphite is an instrument, not a game.
- Glass/blur/frosted materials. Flat, layered, opaque.
- Generic SaaS template layout. The graph visualization IS the layout's identity.
- Em dashes in copy. Semicolons, commas, periods.
- "AI-powered" anywhere. Position on what it does for the agent, not what tech it uses.
