# Table / TEDS gap vs pdf-inspector — progress & followups

Goal: close the table-fidelity (TEDS) gap against **pdf-inspector** (firecrawl) on
`opendataloader-bench` (200 PDFs). Started 2026-08-01.

## Standing

| metric | baseline (2.10.1) | r1 (`093ddba`) | r2 (`520413d`) | r3 (`08225ac`) | r4 (`8aad56b`) | r5 (`09c9080`) | Δ total | pdf-inspector |
|---|---|---|---|---|---|---|---|---|
| **overall** | 0.8732 | 0.8757 | 0.8783 | 0.8786 | 0.8816 | **0.8855** | +0.0123 | 0.8753 |
| TEDS | 0.6929 | 0.7128 | 0.7441 | 0.7471 | 0.7817 | **0.8134** | **+0.1205** | 0.8141 |
| MHS | 0.8114 | 0.8175 | 0.8179 | 0.8179 | 0.8203 | **0.8210** | +0.0096 | 0.7879 |
| NID | 0.9127 | 0.9129 | 0.9136 | 0.9136 | 0.9143 | **0.9169** | +0.0042 | 0.9147 |

ParseBench table composite (the mandatory cross-check): 0.4034 → 0.4063 (r3) →
0.4074 (r4) → **0.4171** (r5), so every round so far is positive-or-flat on
*both* benchmarks.

**After round 5 we lead on every metric.** The TEDS gap is **−0.0007** (was
0.032, and 0.101 before r4) — parity within noise. **TEDS is scored on only 42
of the 200 docs**, so one doc = 0.024 of the mean.

Round 5 is three commits (`bd96364`, `6e1557b`, `09c9080`), ~230 lines in
`tables.rs`, no other file touched. 314 lib tests pass.

## What shipped

1. **`collapse_gutter_columns`** — called from `build_ruled_table` right after
   `cluster_boundaries`. Padded cell borders contribute *paired* vertical edges
   (15–25pt apart), leaving a sliver column between every real column — doc 200 had 13
   columns for a 4-column table. `split_span_at_anchors` shreds text into the slivers, so
   `collapse_phantom_cols` can't see they're empty and the grid dies on the empty-cell
   gate. Fuse a column only when **no raw span center lands inside it** AND it is
   < 0.5× the median *content-bearing* column width.
   *Both halves are load-bearing*: a width-only/bimodal variant broke docs 178+120 from
   1.000 → 0.65, and the width guard is what preserves blank worksheet columns.

2. **Merged-run recovery for absorbed headers** — in `absorb_header_lines`. PDFium emits
   a header row's words as one run, so an 8-col table got a 2-cell header whose blob
   bound to a single column (doc 190). Reuses the existing `recover_merged_cell`.
   Needs **three** gates or it shreds prose into fake headers:
   `absorbed.is_empty()` (first candidate line only), every recovered piece ≤ 30 chars,
   and `cells.len() < column_count`. Dropping any one of these regressed NID.

## What shipped — round 2 (followups 1 + 2)

3. **Two-pass last-resort for header + single-data-row tables** (+0.0188 TEDS,
   +0.0002 NID; doc 197 alone 0.000 → 0.789, *one* doc changed, no collateral).
   `try_detect_table_inferred` takes `allow_two_row`; `two_row_second_pass` retries
   only in the index gaps where the normal pass found no table.
   *The gap restriction alone is NOT enough* — that was the plan and it failed:
   doc 147 still broke (−0.82) and 14 docs lost NID. The real counterfeit is
   **fully-justified prose**, whose stretched inter-word spaces infer as clean
   tracks, so any two consecutive lines shred into
   `| when travelling | to | conflict | zones, | more |`. What actually works is
   `two_row_run_plausible`: **isolation** (neighbours not table-adjacent — a prose
   pair is mid-paragraph) plus **header shape** (cells start uppercase/digit, no
   trailing comma). Gates apply only when the relaxation actually fired, so they
   can never reject something the normal pass accepted.

4. **Multi-line cell / row grouping** (+0.0125 TEDS, 3 docs up, none down;
   doc 150 0.446 → 0.861, now *beating* pdf-inspector's 0.847).
   `merge_continuation_rows`, applied at the end of `merge_table_runs` — the one
   funnel both ruled and borderless runs pass through. Post-hoc and text-only, as
   predicted; the geometry really is unusable. A row folds into its predecessor
   when it has an empty first cell, its filled columns are a **subset** of the
   predecessor's, and at least one extended cell reads as a soft wrap.
   Two vetoes are load-bearing, and **each was found by a different benchmark**:
   - **Any value-like cell → not a continuation** (odl-bench). Numbers don't
     soft-wrap, and a `Total` row has a blank label column precisely because the
     label isn't its own. Vetoing on `all` value-like instead of `any` cost docs
     45 (−0.13) and 47 (−0.09); `any` turned both positive.
   - **First column must be filled in ≥ half the rows** (ParseBench only —
     see the cross-benchmark note below). "Empty first cell" only means "wrapped
     line" if the first column is a *label* column; plenty of tables just have a
     sparse one.

## What shipped — round 3 (word-geometry column splitting)

5. **Split PDFium's merged multi-cell runs on real word geometry**
   (+0.0030 odl TEDS, +0.0029 ParseBench table, +0.0009 NID; docs 189 and 190
   up, **nothing down on either benchmark**).

   This is the root of the "track chicken-and-egg" recorded as the table
   quality ceiling: the detector needs column tracks to split a merged run, but
   the merged runs are exactly where the track evidence lives. Word boxes break
   the cycle — the gutter is visible in the geometry *before* any table
   hypothesis exists. Measured on doc 190's header: in-cell word gaps 1.76pt,
   column gutters 7.7–12.2pt.

   - `split_span_at_gutters(span)` splits one run at its internal gutters;
     `line_pieces(line)` is the tabular view of a row. A run is now what PDFium
     emitted; a *piece* is what the page calls a cell.
   - Wired into all three places that previously reasoned over raw spans:
     `infer_tracks_from_raw_items` (a single merged run now witnesses its own
     columns), `is_strong_row`, and `cells_from_raw_items_with_tracks`.
   - `parser.rs` forces `emit_word_boxes` on for markdown output. Cost measured
     at ~3% user CPU over 40 docs.

   **The threshold must be relative, never absolute.** The same 4pt gap is a
   gutter in 6pt type and an ordinary space in 12pt type — this is the same
   trap recorded in the missing-spaces work. Instead, sort the run's own gaps
   and look for a bimodal jump: real gutters run 3.4–4.4× the in-cell gap,
   while fully-justified prose (the one reliable counterfeit) tops out ~1.3×.
   Requires ≥3 words, since two words give one gap with nothing to compare it
   to. Unit tests cover both the firing case and the justified-prose case.

## What shipped — round 4 (rowspans, the veto share, booktabs bands)

6. **Rowspan-aware density gate** (`87d54ef`; +0.0055 odl TEDS, +0.0011
   ParseBench; doc 146 0.358 → 0.588, nothing down).

   The ruled-grid density gate counted a rowspan continuation cell as a *failed
   text assignment*, so a table with a merged label column ("1. Embodying
   sustainability values" beside three competence rows) died as mostly-empty and
   its text spilled into prose. This is the general form of the "empty-frac is a
   symptom, not a threshold" note below — the empties were real, they just had a
   geometric explanation.

   `rowspan_mask` reads the merge straight off the rules: a cell whose top
   boundary carries **no horizontal stroke over its own centre**, at a boundary
   **some vertical rule runs through**, is merged with the cell above. The mask
   rides on `CellGrid` so it survives `collapse_phantom_rows`/`_cols` (and is
   realigned across `merge_stacked_header`) — an early scalar version forgave
   *pre*-collapse spanned cells against a *post*-collapse empty count and let a
   junk grid through on the scale mismatch alone.

   Both guards are load-bearing, each found by a document it broke:
   - **vertical must continue through the boundary** — otherwise the grid has
     merely *ended* there, and a page-frame component shredded body prose into a
     2-column table.
   - **the merged head must hold text** (walk up the run of spanned cells; the
     first unspanned cell must be filled) — otherwise a chart's vertical
     gridlines made every cell "spanned" and produced a 27-column table,
     −0.0072 NID over 4 docs.

   Test the column *centre*, never containment of the whole column: `xs` comes
   from clustered + gutter-collapsed boundaries and the outer ones can sit tens
   of points off the drawn border (doc 146: `xs[0]=42.9` vs a border at 79.7).

7. **`already_handled` needs a material share** (`46e95b4`; +0.0053 odl TEDS,
   ParseBench flat; doc 200 0.054 → 0.276, NID +0.097, MHS −0.143).

   The global ruled pass builds doc 200's table whole, then `classify.rs` throws
   it away because *some* xy_cut leaf tables on its own. That leaf was the
   landscape slide's title band — **3 of the run's 58 lines**, "tabling" as a row
   of gutter-split title fragments. Require the vetoing leaf to hold ≥¼ of the
   run's lines. This is the mechanism behind the two dead `flatten_header_band`
   guards recorded below: that component was rejected here, downstream of the
   flatten, which is why both guards measured exactly zero.

8. **Booktabs rule-band 2-column pass** (`8aad56b`; +0.0238 odl TEDS,
   ParseBench flat; doc 165 0.000 → **1.000**, matching pdf-inspector, NID
   +0.046, *one* doc changed).

   Item 4's "bordered worksheet" diagnosis was wrong. Doc 165's table is two
   hairlines with **no vertical rules anywhere on the page**, so `extract_h_v_segments`
   yields 0 VSegs (they're gated on `height > 1.0`) and `find_grid_components`
   never forms a component — `build_ruled_table` is never called, and no density
   gate is ever reached. The real killer is `TABLE_MIN_COLUMNS = 3` on a
   genuinely 2-column table. Round 3's splitter already recovers both tracks
   (`ratio=3.42`, xs=[60,118]) from the merged header run; they were discarded
   one line later.

   `rule_bands` pairs two horizontal rules with ≥60% x-overlap, 10pt < gap <
   ½ page, and **no vertical crossing between them** — a band with verticals is
   a real grid and the ruled detector owns it. Inside a band, in gaps the normal
   pass left empty, retry with `allow_two_col`: column minimum 2, and
   single-piece body rows allowed (a blank second column makes *every* body row
   one piece — `cells_from_raw_items_with_tracks` rejects those by default, and
   that guard must stay default-off).

   Detection runs **against the band's lines alone** (sub-slice, indices shifted
   back), so a run can't reach past the rules that justify the relaxation. Doing
   it in-place instead failed on `row spacing cv 0.53` because the run swallowed
   the paragraph below the band. Further gates: band lines contiguous, ≥3 of
   them, seed line reads as exactly 2 pieces.

## What shipped — round 5 (the three things nobody was reading off the page)

Every round-5 change has the same shape: the geometry already answered the
question and no code was looking at it.

9. **`RULED_VLINE_MIN_COVERAGE` — vertical rules too short to be columns**
   (`bd96364`; +0.0040 odl TEDS, ParseBench flat; doc 200 0.276 → 0.445).

   A slide deck draws a table as one stroked rect *per cell*, so a 21.6×9.4pt
   highlight box behind a phrase **inside** a cell contributes a pair of
   vertical edges that split that column for the whole table — doc 200's 210pt
   `Explanation` column became three, taking a 4-column grid to 6.
   `collapse_gutter_columns` cannot fuse the sliver back, because text centres
   genuinely do land in it. The mirror of `RULED_HLINE_MIN_COVERAGE`: a vertical
   must span 20% of the component's row extent.

   **Applying the filter directly is what does not work, and this is the
   general lesson: dropping evidence can flip a grid from rejected to
   accepted.** Three documents found that one at a time — a bar chart lost its
   last column and released its gridlines as thematic breaks (doc 71, MHS
   0.98 → 0.62); a pie chart's callout rules were dropped and a grid the density
   gate had rejected became a junk 2-column table (doc 70, NID −0.10); and
   ParseBench `FBLB-134215544_page88` turned a rejected component into an
   11-column table that swallowed the page (0.66 → 0.03) after dropping **one
   4%-coverage stub out of 20**. Raising the fallback threshold patched each in
   turn and kept finding new ones. What kills the class is structural: the
   unfiltered build runs first and gatekeeps, so the filtered grid can only
   ever *replace* a table that would have been produced anyway.

   It must also *earn* the replacement. `CellGrid` now carries `straddle_frac`,
   and the refined grid is taken only if the straddle census drops by ≥0.05 —
   doc 200 goes 0.33 → 0.03, whereas a stub sitting on a real column edge (very
   common in dense financial tables) leaves it flat and merely costs a column
   (ParseBench `corp-q1-2025_page10`, 0.73 → 0.33 without this).

10. **Unruled outer rows and columns, recovered from the perpendicular rules**
    (`6e1557b`; +0.0123 odl TEDS, **+0.0070 ParseBench** — the largest
    single-change ParseBench gain so far; doc 182 0.247 → 0.762).

    A table with interior dividers but no outer frame gives `xs` that stop at
    the first and last vertical rule, so its outermost columns are *missing*.
    Doc 182's 4×4 table came out as the 2×2 grid its rules literally describe
    and all 31 lines tripped the overhang guard before the component collapsed.
    The horizontals already know how wide the table is and the verticals how
    tall; nothing read them. Extend each axis to the **median** extent of the
    perpendicular rules.

    Three guards, each load-bearing:
    - **median, not min/max** — one overshooting stroke must not widen the grid;
    - **the new band must hold a span centre**, the `collapse_gutter_columns`
      idiom, or a pen-cap overshoot manufactures an empty outer column;
    - **the band must be at least as tall as the shortest existing row.**
      Verticals routinely overrun the last horizontal by a few points; doc 45
      turned a 15.5pt tail on 26pt rows into a phantom row, which was enough for
      the ruled grid to outrank a better borderless table and merge five rows
      into one (−0.50). The *column* axis takes no such floor — an unruled label
      column is common and legitimately much narrower than its data columns
      (doc 182's is 94pt against 259pt).

    On ParseBench four pages of one insurance filing go from 0.09–0.42 to ~0.99.

11. **Split a grid component at row bands no vertical rule spans**
    (`09c9080`; +0.0154 odl TEDS, +0.0027 ParseBench; docs 82/84 → 1.000,
    83 → 0.995, 81 → 0.935).

    This closes followup 8, and the mechanism was **not** the one recorded
    there. `cluster_v_segments` merges same-x verticals by taking the *union* of
    their y-ranges **with no gap check**. Two tables stacked in one column, each
    drawing its own short strokes at the same left edge, therefore fuse into one
    component across 113pt of blank page. Each is then scored with the other's
    rows empty and dies on the empty-cell fraction — and where the two layouts
    differ their column sets are unioned too. Doc 81's `Number | of clauses`
    split across two columns, filed in the negative results as a
    detector-priority problem, is that union and needs no priority plumbing.

    The cut signal needs no emptiness threshold: a band between consecutive
    horizontals that **no raw, pre-cluster `VSeg` spans**. That is strictly
    stronger than "a big gap", and unlike a gap rule it cannot fire on a
    rowspan, where the vertical does continue through the boundary. Each band's
    verticals are then **clipped to the band**, or the shared spine carries the
    neighbour's height straight back in through items 9 and 10.

    Guards: median row pitch ≥ 8pt (vector-drawn maths glyphs make components
    with a 3–4pt pitch), gap ≥ max(2.5 × pitch, 30pt), and every band must keep
    ≥2 rows or the split is abandoned whole. Docs 127/130/165/188 — the ones the
    round-4 trim experiment broke — are untouched, because a post-hoc row trim
    keeps the unioned *columns* and splitting the component does not.

    **Requiring 3 rows per band instead of 2 was measured and is worse**
    (−0.0004 ParseBench: it loses `FBLB-134215544_page6` and recovers neither of
    the two docs it was aimed at). Left at 2.

## Cross-benchmark check is mandatory for table work

**opendataloader-bench alone is not enough.** Round 2 looked perfect on it — four
docs up, none down — while silently costing **−0.0048** on the ParseBench table
dimension. Attribution (one run per variant):

| variant | odl TEDS | ParseBench table composite |
|---|---|---|
| HEAD `093ddba` | 0.7128 | 0.4032 |
| + two-row pass | 0.7316 | 0.4032 (free) |
| + continuation merge | 0.7441 | **0.3984** ← regression |
| + first-column fill guard | 0.7441 | **0.4034** |

The continuation merge owned the whole regression, concentrated in a few docs
(`1 timetable_page6` 0.859 → **0.006**, `sizingchart`, `myco hierarchical table
header`). All were tables with a legitimately sparse first column, where every
row reads as a continuation and the table collapses to one row. The fill guard
fixes them at zero cost to odl-bench.

Note `is_value_like` does **not** match bare times (`8:24`) or bare integers
(`15`) — it needs `d.d` / `d,d` / `$` / `%`. Don't widen it to patch a table bug;
it gates several unrelated paths. Prefer a structural guard.

Run: `cd ParseBench && uv run parse-bench run liteparse_markdown --group table`
(~12 min, 503 examples). Read `avg_grits_trm_composite` from
`output/liteparse_markdown/_evaluation_report.json` — this is the leaderboard's
"Tables" column (0.4032 = the 40.3 in `leaderboard.csv`). **The run overwrites
that report in place**, so copy it aside before running a variant.

## Tooling

- **Per-doc deltas**: `opendataloader-bench/perdoc.py <label-b> [label-a] [metric]`
  — prints every doc whose metric moved between two prediction labels, worst
  first. Doc numbers match the ones used in this file. This is what caught that
  the round-2 mean hid a −0.82 doc in the first two-row attempt.
- **A/B harness**: `opendataloader-bench/ab.sh <label>` — parses all 200 PDFs with the
  tuned bench config and prints the delta vs `prediction/liteparse` (the frozen 2.10.1
  baseline). Verified byte-identical to the recorded baseline on all 200 docs.
  Takes ~1–2 min. **Always A/B; per-doc deltas matter more than the mean.**
- **Geometry dumps** (`crates/liteparse/examples/`, run with
  `cargo run -q --release -p liteparse --example <name> -- <pdf>`):
  `dump_rects_raw` prints every graphic primitive plus each text item with its
  word boxes and inter-word gaps — this is what made the gutter signal visible;
  `dump_chars_raw <pdf> <page> <ymin> <ymax>` drops to per-character boxes.
  `dump_table_rects` gives the h/v stroke counts per page, which is how you tell
  a booktabs table (`h_strokes=6 v_strokes=0`) from a fully ruled one.
  Beware: vector-drawn math glyphs show up as ~170 tiny `stroked_rects` and can
  produce a phantom table rect (doc 165).
- **Debug envs**: `LITEPARSE_DEBUG_TABLE=1 LITEPARSE_DEBUG_RULED=1` on a single
  `lit parse ... --format markdown --image-mode off --no-ocr --no-links -q`.
  These print every gate rejection with its numbers. **Use these first** — twice I
  inferred a mechanism from the emitted markdown and was wrong.
  `LITEPARSE_DEBUG_GUTTER=1` prints every in-run column split with its ratio.
- **Never rebuild the binary while a bench run is in flight** — both harnesses
  shell out to `target/release/lit`, so a mid-run `cargo build` silently mixes
  two variants into one score.
- **Attribution runs are cheap and mandatory.** Two changes bundled into one
  ParseBench run cost a day of ambiguity in round 4; one run per variant
  (~12 min) is always worth it. `ab.sh` on odl is 1-2 min, so bundle there and
  split on ParseBench.
- **pdf-inspector repro**: `git clone --depth 1 https://github.com/firecrawl/pdf-inspector`,
  `cargo build --release --bin pdf2md` (~28s), run `pdf2md <pdf> --raw`. Its outputs and
  scores are already committed at `opendataloader-bench/prediction/pdf-inspector/`.
  Re-evaluate with `uv run src/evaluator.py --engine pdf-inspector`.

## Where the remaining −0.0007 sits

We are at parity on the mean, so from here the question is no longer "close the
gap" but "which docs are still individually behind". Top per-doc gaps after
round 5 (perdoc index, ours vs pdf-inspector):
199 −0.346, 118 −0.276, 196 −0.211, 120 −0.173, 45 −0.156, 189 −0.127,
145 −0.126, 169 −0.122, 115 −0.108, 165 −0.107, 187 −0.083, 80 −0.065.

Note the index shift: these are `perdoc.py` indices (doc id = index + 1). The
old "doc 200 / 83 / 81" labels in the sections above are ids; 199/82/80 are the
same documents.

Docs we *lead* on offset all of this, which is why the mean is level — but
every entry above is still a real defect.

## Where the 0.101 sat before round 4

Measured by GT-cell-text retention (liteparse **0.747** vs pdf-inspector **0.870**):

| class | TEDS cost | meaning | docs |
|---|---|---|---|
| **MISPLACED** | −0.073 | text is in the doc but never lands in a table | 165, 197, 200, 182, 116 |
| **STRUCTURE-only** | −0.058 | text *is* in the table, structure wrong | 150, 146, 119, 190, 81, 84, 166, 188, 82, 90, 89 |
| LOST-upstream | −0.008 | genuine extraction failure | 121, 46 |

So the dominant problem is **misfiling**, not dropping, text.

## Followups, ranked

*(followups 1 and 2 are done — see "round 2" above.)*

2b. **More of the continuation class is still available.** `merge_continuation_rows`
   only fires on rows with an **empty first cell**. pdf-inspector's veto set is richer
   (`looks_like_data_row`, `is_short_subheader`, `looks_like_hierarchical_subrow`,
   `looks_like_spanning_first_column_row`), which lets them merge wraps where the first
   column *is* populated. Worth trying next, but the empty-first-cell rule is what makes
   the current version regression-free — relax it only behind an A/B.

3. **Doc 200 / index 199 — the columns are fixed, the rows are not**
   (0.445 vs 0.791, ~+0.008 available). Item 9 fixed the column
   over-segmentation this slot used to describe; `xs` is now GT's 4. Two things
   remain, and they are one mechanism:
   - The page is **not a ruled grid at all — it is 44 independent per-cell
     rects** (4 distinct x/width pairs × 11 rows; see `dump_rects_raw`). The
     columns' rects are not y-aligned (each cell's rect hugs its own text), so
     unioning every top and bottom yields 24 row boundaries for 11 rows. Tall
     cells then get cut by a neighbouring column's boundaries and their text
     splits across rows (`2. Data labeling and` / `fine-tuning`).
   - `flatten_header_band` still folds the two full-width *title* banners into
     the header row.
   The right model for this class is to cluster the cell rects directly —
   columns by near-identical (x, width), rows by **y-interval overlap** (rows
   overlap ~100%, adjacent rows 0%, which is unambiguous where no y threshold
   is). That is a new detector, not a tweak, and it needs priority plumbing
   against the ruled path.
   A cheaper partial: `rowspan_mask` already marks these cells correctly (it is
   how the density gate survives), but `spanned` is only *counted*, never used
   to merge cell text. Making a spanned cell fold into the cell above is
   contained and reuses round-4 machinery.
   Do **not** relax the `flatten_header_band` anchor — measured at −0.0043 on
   ParseBench, see negative results.

4. ~~**Doc 165 worksheet**~~ — **DONE in round 4** (item 8), 0.000 → 1.000. The
   "bordered worksheet / `TABLE_MAX_EMPTY_CELL_FRACTION`" diagnosis in this slot
   was wrong; see item 8 for what it actually was.

5. ~~**Tagged `/Table` structure tree**~~ — **DEAD. Do not build this.** Measured
   before writing any code: **0 of the 200 opendataloader-bench PDFs carry a
   structure tree at all**, and 0 of the first 120 ParseBench docs do either.
   A tagged-`/Table` detector would be pure dead code against every benchmark we
   optimize. The extractor is fine — verified against a LibreOffice-exported
   tagged PDF, which returns the full `Document > Table > TR > TH/TD` tree — the
   corpora are simply untagged. pdf-inspector's lead comes from somewhere else.
   Probe: `lit parse <pdf> --format json --extract-structure-tree` and count
   nodes under `pages[].structure_tree.roots`. Still worth having for *real
   user* documents someday, but it can never show up in these numbers.

6. ~~**Sub-word tracks**~~ — **built and measured inert.** `split_words_at_x_anchors`
   (real word x's) and `bucket_words_into_columns` (per-word ruled-column
   assignment) now front both anchor-split paths, and the 200-doc output is
   **byte-identical**. The borderless multi-track branch fires only **12 times in
   the whole corpus** and word/char agree every time; the ruled branch fires 434
   times and buckets 219 of them differently, but every one of those components
   is rejected later anyway. Round 3's gutter split already consumes this class
   upstream. Kept (geometry beats interpolation where it does fire), but don't
   expect a number from it — doc 83's `10 .1%` comes from somewhere else.

7. **Doc 190's last row (`Merge v4 | SLERP`, −0.126).** The header row is fixed;
   the final body row still falls out because its second cell (`SLERP`, x=182.9)
   sits 14pt off the track inferred from the rows above (`Average`, x=168) —
   past the 6pt `TABLE_TRACK_TOLERANCE_PT`. Needs track *ranges* rather than
   points; touch with care, that tolerance gates several paths.

8. ~~**Docs 81/83/84 (same source family)**~~ — **DONE in round 5** (item 11).
   The diagnosis in this slot was right about the symptom and wrong about the
   cause: the component does span two stacked tables, but because
   `cluster_v_segments` unions y-ranges across a 113pt gap, not because of
   anything about the rows. Splitting the component (not trimming rows) fixes
   all four and leaves the trim experiment's casualties alone.

9. **Doc 119 / index 118 (0.724 vs 1.000, ~+0.007).** Diagnosed, not built. Two
   independent defects:
   - **9 phantom rows.** The PDF draws a background fill rect per *text line*
     inside each cell, and `RULED_HLINE_MIN_COVERAGE` is **Global-pass only** —
     the per-region pass (which is what emits this table) keeps every
     horizontal. Coverage over the column extent is perfectly bimodal: 7
     boundaries at ≥0.998 (exactly GT's 6 rows) against 9 phantoms at 0.661 and
     0.336. Note the existing 0.5 threshold would *not* fix it even if promoted;
     it needs a strict tier (~0.9) gated on the full-width frame being real.
     **Blast radius is measured and large**: over the 200-doc corpus, 24 docs
     lose boundaries under a 0.9 filter, including 165, 150, 199, 200 and 141.
     This needs its own attribution run and a hand check of 165.
   - **A wrong column split.** `bucket_words_into_columns` returns `None` when
     every word centre lands in one column — the doc comment says the caller's
     "whole-span fallbacks" are then the honest answer, but the actual next
     fallback is `split_span_at_anchors`, which *interpolates* a split from a
     character index and cuts `(begins` off into the previous column. Word
     geometry had the right answer and it was discarded precisely because it was
     unanimous. Small, independent, and worth doing first.

10. **Doc 197 / index 196 (0.789 vs 1.000) — do not chase this to 1.0.**
    Measured by patching the markdown and re-scoring: the *faithful* structural
    reading of the missing table (1 column × 11 rows) scores **0.447**, i.e.
    −0.34. GT's 1.000 requires reproducing an idiosyncrasy (a header cell and a
    merged body cell side by side in one row). A 1-column header + single merged
    body cell gets 0.850 (+0.061 on one doc = +0.0015 mean) and would require a
    new 1-column-table producer in `two_col_band_pass`. Low priority, and the
    ceiling is an artefact rather than a defect.

## Negative results — do not retry blind

- **Relaxing `TABLE_MAX_EMPTY_CELL_FRACTION`** (swept 0.45/0.60/0.75): 0.45 neutral,
  0.60 −0.013 TEDS, 0.75 −0.040. `empty-frac` in the gate logs is a *symptom* of text
  not being assigned, not a tight threshold.
- **Relaxing the straddle-frac gate (0.45)**: zero TEDS gain, costs NID and MHS. High
  straddle was a symptom of column over-segmentation — it fell 0.48 → 0.32 by itself
  once gutters collapsed.
- **Full-width-spanner exclusion in `flatten_header_band`**: exactly zero delta, guard
  never fires (see item 3).
- **Directly allowing 2-row header-led tables**: fixes doc 197 (+0.789) but breaks doc
  147 (−0.822) plus ~8 NID regressions. The existing code comment is right — a 2-row run
  forms early and consumes the real table's header.
- **Restricting the 2-row relaxation to table-free gaps, with no content gate**:
  doc 147 still −0.822, 14 docs lose NID (−0.0030). The gap restriction addresses the
  wrong mechanism. Justified prose is the counterfeit, and it lives *in* the gaps.
- **Trimming fully-empty edge rows off a ruled grid** (aimed at docs 81/83/84,
  where one component covers two stacked tables so each sees the other's rows as
  empty). Tried four increasingly-gated variants; all net-negative or a wash:
  | variant | odl TEDS vs round 3 |
  |---|---|
  | trim unconditionally | −0.0122 (docs 130, 127, 81 fall from ~1.0 to ~0.75) |
  | trim only if the untrimmed grid fails the density gate | identical — the pre-check is not predictive, `flatten_header_band` / `merge_stacked_header` rescue those grids *later* |
  | retry trimmed only after the untrimmed build returns `None` | identical again |
  | + strict density on the retained block, + require ≥3 trimmed rows and ≥4 kept | −0.0006 (83 +0.084, 84 +0.066, 81 −0.119, 82 −0.055) |

  The mechanism the gating missed: the trimmed run doesn't fill a hole, it
  **outranks a better table another detector already produced**. Docs 81/127
  regress because the ruled run wins with one column too many
  (`Number | of clauses` split across two columns) over a correct borderless
  table. Fixing this class needs either detector-priority plumbing or splitting
  the *component* — not a post-hoc row trim. Reverted; not in the shipped diff.
  **Round 5 settled this**: splitting the component (item 11) was the answer,
  and the extra column was the two tables' x-sets being unioned, so no
  detector-priority plumbing was needed after all.
- **Filtering short vertical rules before the grid is built** (round 5, item 9).
  Three separate documents turned a *rejected* grid into an accepted junk table
  because the filter removed the evidence the gates were rejecting it on.
  Raising the fallback threshold (2 → 3 → 4 surviving rules) patched them one at
  a time and kept finding new ones. Never let a cleanup pass run before the
  gatekeeper.
- **Requiring 3 rows per band in the component split** (round 5, item 11):
  −0.0004 ParseBench, loses `FBLB-134215544_page6`, recovers neither doc it was
  aimed at. Left at 2.
- **Tagged-`/Table` and sub-word tracks** — see items 5 and 6 above; both were
  measured inert *before* being built out, which is the cheapest thing in this
  whole document. Do that first.
- **Relaxing the `flatten_header_band` anchor** to accept a half-filled row of
  short (≤40 char), column-distinct labels when column over-segmentation pushes
  the real header below `TABLE_ROW_MIN_FILL`. Doc 200 +0.009 TEDS / +0.020 NID
  with no odl collateral — and **−0.0043 on ParseBench**, attributed by a
  one-variable run. Reverted. This is the second time odl-bench alone would have
  shipped a table regression.
- **A second full-width-spanner guard in `flatten_header_band`** (reject the flatten
  when a produced header cell exceeds 100 chars, aimed at doc 200): **exactly zero
  delta** — this is the second guard to die here. The `[ruled] colspan header flatten`
  line you see in doc 200's debug output belongs to a component that is *rejected
  later anyway*; the emitted 5-column table comes from a different component. Before
  touching `flatten_header_band` again, confirm the component you're looking at is the
  one that actually reaches the output.

## Methodology gotchas (both bit me)

- **Gate-reason histograms over the corpus are mostly noise.** `tracks < MIN_COLUMNS`
  fires on every prose line; `grid-too-small ys=2 xs=2` is the page-background rect;
  `no-lines-consumed` is every non-overlapping region/component pair. Rank by per-doc
  loss instead, then read that doc's debug output.
- **`difflib.SequenceMatcher` needs `autojunk=False`.** The default silently corrupts
  comparisons on strings over 200 chars and produced impossible numbers (a doc's table
  text matching better than the whole document containing it).
- Table detection runs on region lines **before** interruptions (HRs, figures) are
  interleaved (`classify.rs:499`), so thematic breaks do *not* fragment table rows —
  an early theory of mine that the debug output disproved.
