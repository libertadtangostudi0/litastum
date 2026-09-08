# litastum: popup design

## Shared primitive: `ui/popup.rs`

Redesigning the F9 menu, color-scheme picker, and delete-confirm popups
against a reference mockup (`litastum-popups-temp-concept.html`, not
committed) turned up one shared visual language across all three:
title as its own line (not baked into the border), a dim horizontal
`separator` rule before the footer, and footer hints styled as filled
"pill" badges (`key_pill`) rather than plain colored letters. Pulled
into `ui/popup.rs::{draw_frame, separator, key_pill}` instead of
duplicating the same chrome three times — `ui/confirm.rs`'s delete
popup and `ui/theme_menu.rs`'s color-scheme picker already use it.

**Any new popup should build on this primitive**, not hand-roll its own
`Block::default().borders(Borders::ALL)` the way every popup did
before this (`ui/menu.rs`'s F9 top menu still does, as of this
writing — next in line to move over).

## Settled: rounded border, no background fill, uniform padding

This took a long back-and-forth to reach, entirely about one thing:
`BorderType::Rounded`'s corner glyphs (`╭╮╰╯`) are Unicode
line-drawing characters that *suggest* a curve, but the character
*cell* underneath is always a hard square — no background fill can
clip itself to that curve the way a real CSS `border-radius` (what the
reference mockup actually used) can. Every attempt at working around
this was tried, in this order, and rejected:

1. **Solid `theme.surface` fill** (a distinct, lighter shade than the
   panels, for a "raised card" look) — the square cell under each
   rounded corner glyph was plainly visible against it, reported
   directly ("прямоугольник выходящий за пределы рамки").
2. **Solid `theme.bg` fill instead** (same color as the panels, so the
   square would blend in) — still visibly separate, because *nothing
   else in this app ever explicitly paints `theme.bg` anywhere either*.
   Every panel just draws text over the terminal's own untouched
   default background, never filled — so `theme.bg` had no guarantee
   of matching that default, and didn't.
3. **No fill at all** (`Clear` only, relying on the untouched default
   matching the surroundings exactly) — fixed the corner mismatch, but
   gave up the filled-card look entirely; also rejected.
4. **Fill everywhere except the 4 literal corner cells** (left at the
   untouched default) — a single unfilled cell doesn't read as
   "rounded," just as a small notch; rejected.
5. **A diagonal quadrant-block glyph at each corner** (▗▖▝▘, replacing
   the border's own corner character with a 45° chamfer) — explicitly
   reported as looking *worse* than every previous attempt.

**Where this landed**: `BorderType::Rounded` with a full `theme.bg`
fill (step 2's approach) is the current, accepted state — not because
the corner mismatch is gone (it structurally can't be, in a character
grid), but because it's what looks least bad after exhausting the
realistic alternatives. A character cell is simply not enough
resolution to render an actual curve by any means a terminal offers;
this is a hard limit of text-mode UI, not specific to `ratatui` or fixable
by trying yet another glyph combination. **Do not re-litigate this**
without a genuinely new idea, not a variation already listed above.

`PADDING` (the gap between the border and content on all four sides)
is `Padding::uniform(2)` — not the `+2` horizontal / `+1` vertical
`ratatui::widgets::Padding` itself recommends for *visually equal*
padding (compensating for terminal cells being roughly twice as tall
as wide). Uniform cell counts were requested specifically so the gap
measures the same in both directions at the rounded corner itself.
Confirmed equal on all four sides by
`ui/popup.rs::tests::the_gap_between_border_and_content_is_equal_on_every_side`,
which measures `draw_frame`'s actual returned `inner` rect against an
independently-computed popup rect, not just that `PADDING`'s literal
fields match. Note: equal *cell counts* still doesn't look equal to
the eye, for the same cell-aspect-ratio reason above — tried anyway,
per explicit request, and accepted as close enough.

## Also settled while redesigning these three

- **Footer hint row is centered** (`Line::centered()`) in the
  color-scheme picker — matches the reference mockup. The delete
  popup's own hints are left-aligned instead, matching *its* mockup
  row — these were reviewed as two different reference screenshots, so
  don't assume one alignment rule applies to every popup's footer.
- **A selected list row's highlight is inset by one unstyled column on
  each side**, not a full-bleed bar — approximates the reference
  mockup's rounded "pill" selector (an actual curve is just as
  unrenderable here as the border's, so a 1-column gap stands in for
  it). Implemented as manual per-row background spans
  (`ui/theme_menu.rs::theme_row`), not `List::highlight_style` — that
  API always paints the *entire* row_area regardless of content width,
  so it can't produce an inset. Every row (selected or not) reserves
  the same leading/trailing column, or content would visibly shift
  sideways the moment a row becomes selected.
