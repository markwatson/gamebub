# UI pagination — undecided

Notes on how L/R paging in `FileListViewBase` should behave. Currently
unresolved; the shipped behaviour is option A below, chosen mostly because it
was the simplest thing that fixed the original bug.

## Where it stands

`res/ui/components/file_list.slint`. `focus-page-up`/`focus-page-down` move the
selection by `page-size` and clamp at the ends. `bring-into-view` scrolls
*minimally*, so the cursor lands at the **leading edge** of travel — bottom on
page-down, top on page-up.

Note these two functions existed before but were never wired to anything; the
dead paging code went to the viewport edge first, which is why it needed two
presses to move a full page.

## Two independent knobs

Most of the confusion here comes from conflating these.

1. **Step size** — `page_size` (disjoint pages) or `page_size - 1` (one item
   carried across the boundary as an anchor).
2. **Cursor placement after the jump** — leading edge (free, from minimal
   `bring-into-view`), preserved screen row, or forced to top.

## Options

**A. Uniform `page_size`, leading-edge cursor** — current. Disjoint,
contiguous pages, nothing skipped. Simplest.

**B. Uniform `page_size - 1`, leading-edge cursor** — same, with one item
visible in both the old and new view. Closest to the documented convention for
list widgets (W3C ARIA listbox guidance describes page-down as scrolling so the
last option in the current view becomes one of the first in the new view), and
to `vi` Ctrl-F / Emacs `next-screen-context-lines`, both of which keep a couple
of lines of context. Two-line change: add
`page-step: max(1, page-size - 1)` and use it in the two call sites.

**C. Edge-first, then page** — the original dead code. **Rejected.** The first
press moves a variable distance depending on where the cursor sits, so you can
never build a "three taps gets me to the S's" reflex. It is the loss of
countability, not the lower throughput, that kills it.

**D. Preserve cursor screen row** (Miyoo Mini) — index moves by a full page
either way; only the visual differs. Content flows past a stationary cursor.
Needs explicit scroll positioning rather than `bring-into-view`.

**E. Jump to letter** — what Pokémon actually implies. It uses L/R to switch
*containers* (PC boxes, bag pockets), never to page within a list; its lists are
short or partitioned so the problem never arises. For a large ROM library the
equivalent would be alphabetical jumps. A different feature, not a tweak.

## What matters most

**Predictability, not throughput.** Speed on these devices comes from muscle
memory — same input, same displacement, every time. Both A and B have it; C does
not.

One wrinkle: Miyoo's list always starts at the top, which is what makes absolute
muscle memory work there. Game Bub restores the last-played position, so you
cannot count from a fixed origin and end up visually scanning instead. That
slightly favours B, since an anchor item helps when reading boundaries rather
than counting presses.

## Separate axis: scroll behaviour

Pokémon keeps the cursor **centred** while scrolling — items shift around it,
and the cursor only breaks to an edge at the ends of the list. This is vim's
`scrolloff`. Game Bub does the opposite: the cursor walks to the edge and stays
pinned there, so there is no look-ahead in the direction of travel.

This affects up/down, not L/R, and is independent of everything above. Worth
considering on its own if edge-pinning starts to grate.

## Next step

Live with A for a while. A week of actually browsing the library will settle
this better than more analysis, and switching to B is two lines.
