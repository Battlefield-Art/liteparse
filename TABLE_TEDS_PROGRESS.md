# Table / TEDS gap vs pdf-inspector — progress & followups

Goal: close the table-fidelity (TEDS) gap against **pdf-inspector** (firecrawl) on
`opendataloader-bench` (200 PDFs). Started 2026-08-01.

## Standing

| metric | baseline (2.10.1) | round 1 (`093ddba`) | round 2 (`520413d`) | round 3 | Δ total | pdf-inspector |
|---|---|---|---|---|---|---|
| **overall** | 0.8732 | 0.8757 | 0.8783 | **0.8786** | +0.0054 | 0.8753 |
| TEDS | 0.6929 | 0.7128 | 0.7441 | **0.7471** | **+0.0542** | 0.8141 |
| MHS | 0.8114 | 0.8175 | 0.8179 | **0.8179** | +0.0065 | 0.7879 |
| NID | 0.9127 | 0.9129 | 0.9136 | **0.9136** | +0.0009 | 0.9147 |

ParseBench table composite (the mandatory cross-check): 0.4034 → **0.4063** in
round 3, so this round is positive on *both* benchmarks.

We lead on overall + MHS, trail on TEDS by **0.067** (was 0.101) and NID by 0.001.
**TEDS is scored on only 42 of the 200 docs**, so one doc = 0.024 of the mean.

Round 1 shipped as `093ddba`, round 2 as `520413d`; round 3 is ~120 lines in
`crates/liteparse/src/markdown_layout/tables.rs` plus one line in `parser.rs`.
303 lib tests pass.

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
- **pdf-inspector repro**: `git clone --depth 1 https://github.com/firecrawl/pdf-inspector`,
  `cargo build --release --bin pdf2md` (~28s), run `pdf2md <pdf> --raw`. Its outputs and
  scores are already committed at `opendataloader-bench/prediction/pdf-inspector/`.
  Re-evaluate with `uv run src/evaluator.py --engine pdf-inspector`.

## Where the remaining 0.101 sits

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

3. **Doc 200 (−0.737 alone, ~+0.018).** Columns are now correct after the gutter fix,
   but the body rows come out empty. Cause: `flatten_header_band` absorbs the landscape
   slide's full-width title rows into the colspan header, so rows 0..3 become a junk
   header band. Note the obvious guard (skip rows whose `cells_repl` is uniform across
   all columns) **does not fire** — PDFium splits the title into several spans.

4. **Doc 165 worksheet (~+0.024).** Bordered worksheet whose second column is entirely
   blank; the ruled grid is rejected (`rows-after-collapse 1`, `no-lines-consumed`).
   Related: `TABLE_MAX_EMPTY_CELL_FRACTION` cannot be relaxed (see below), so this needs
   a different route — likely the tagged struct tree (item 5).

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

6. **Sub-word tracks are still on the table.** Round 3 uses word boxes only to
   split runs into pieces. `split_text_at_x_anchors` still locates a split by
   *linear interpolation over character index*, which is why doc 83 emits
   `10 .1%`. Feeding real word x's into that function is the obvious next step.

7. **Doc 190's last row (`Merge v4 | SLERP`, −0.126).** The header row is fixed;
   the final body row still falls out because its second cell (`SLERP`, x=182.9)
   sits 14pt off the track inferred from the rows above (`Average`, x=168) —
   past the 6pt `TABLE_TRACK_TOLERANCE_PT`. Needs track *ranges* rather than
   points; touch with care, that tolerance gates several paths.

8. **Docs 81/83/84 (same source family, −0.64 summed).** One grid component
   spans two stacked tables, so each is evaluated with the other's rows empty
   and both die on `empty-frac 0.60`. See the trim experiment in negative
   results for what does *not* work.

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
