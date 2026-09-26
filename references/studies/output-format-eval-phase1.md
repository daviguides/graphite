# Output format eval — phase 1

Question: which way of expressing **graph + ripgrep** results lets the model answer code-change questions most accurately, with the fewest input tokens?

Branch `exp/format-eval` only (not merged). Harness: `bench/format-eval/` (`extract.py`, `render.py`, `eval.py`). Records and raw results are gitignored (derived from the private Continuum repo). Rerun: `FE_REPO=<continuum copy> python3 extract.py && python3 eval.py run && python3 eval.py report && python3 eval.py analysis`.

## Setup

- **Data is real.** Every match comes from Graphite's embedded ripgrep search (`graphite-hook run --all`) on a copy of Continuum. The graph relations come from `graphite blast --json --all` (built from main `68ec468` into `target/dev-format-eval`). Nothing was synthesized.
- **The record is match-centric (integrated).** Each real match appears once in a single list, with the graph's annotation: definition, call (enclosing fn, its callers, which definition it resolves to), import, mock in test, docs, string/comment, unresolved call (N candidates), untracked code use, and so on. Graph facts with no text match appear in the same list as `graph-only`: the 7 MeetingRegistry.get callers outside the searched directory. Non-line facts go in a header (query, verdict and causes, target, counts, candidates) and a footer (indirect impact, covering tests, overrides, hidden-dir omissions, budget-cut counts).
- **Every format gets the same budget.** At most 60 items, ranked by definition, call, graph-only, unresolved, mock, docs, string, import. Items beyond the budget are counted per class in the footer. All 9 formats show the same items. Nothing is byte-cut.
- **Cases (6):**
  - `resolve_owner`: complete, with mocks and docs mentions;
  - hub `load_yaml`: 3 definitions, 452 matches, compressed;
  - `BaseCollector.collect`: overrides, lower bound, 0 resolved callers;
  - `_pin_model`: 8 definitions;
  - `MeetingRegistry.get`: lower bound, graph-only callers outside the search path;
  - `except Exception`: non-identifier, grouped by enclosing symbol.
- **Questions:** 25, with ground truth derived from the record and scored deterministically (list F1, exact match for booleans, integers and choices). Answers are JSON plus one evidence quote per question. There is no LLM judge.
- **Model:** sonnet via `claude -p --tools "" --system-prompt …`. 9 formats × 6 cases × 3 repeats = 162 calls.

### How residue is rendered in each format (the same integrated list, 9 shapes)

| # | Format | Matches and residue |
|---|---|---|
| 1 | lines (`grep -rn` shape) | `path:line:text    [class; in fn; called by …; resolves to …]`; header/footer as `#` lines |
| 2 | grouped (`rg --heading`) | path header, then `line:text [class; …]` per match |
| 3 | tree | target → file → enclosing fn (← callers) → `L<line>: text [class]` leaf |
| 4 | XML | `<matches><m at cls in called_by resolves_to candidates>text</m>…`, header attributes, `<footer><fact>` |
| 5 | JSON | one `matches` array with `class` and relation fields per item, plus header and footer objects |
| 6 | YAML | same structure as JSON |
| 7 | edges | `<fn> CALLS / IMPORTS / MOCKS / MENTIONS / UNRESOLVED_CALL / USES / CONTAINS_MATCH <target> @ path:line: text`, `X CALLS <fn>` for callers, `FACT` lines |
| 8 | Mermaid | one edge per match: solid for call/definition/import/match, dashed `-.->` for mock/docs/string/unresolved; header/footer as `%%` comments |
| 9 | prose | one sentence per (file, class) group, header and footer as sentences |

## Results

| format | accuracy | sd across repeats | input tokens | chars | list_loc | list_path | list_names | bool | int | choice_path | depends | unparsed | n |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| f1_lines | 1.000 | 0.000 | 5086 | 9122 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0 | 18 |
| f4_xml | 1.000 | 0.000 | 5816 | 10275 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 0 | 18 |
| f5_json | 0.989 | 0.016 | 6082 | 11214 | 1.00 | 1.00 | 1.00 | 0.92 | 1.00 | 1.00 | 1.00 | 0 | 18 |
| f6_yaml | 0.981 | 0.026 | 6293 | 11964 | 1.00 | 1.00 | 1.00 | 1.00 | 0.94 | 1.00 | 1.00 | 0 | 18 |
| f7_edges | 0.972 | 0.020 | 6454 | 11364 | 1.00 | 1.00 | 1.00 | 1.00 | 0.89 | 1.00 | 1.00 | 0 | 18 |
| f3_tree | 0.958 | 0.000 | 4738 | 8306 | 1.00 | 1.00 | 1.00 | 1.00 | 0.83 | 1.00 | 1.00 | 0 | 18 |
| f8_mermaid | 0.950 | 0.012 | 5942 | 10060 | 0.97 | 1.00 | 1.00 | 1.00 | 0.83 | 1.00 | 1.00 | 0 | 18 |
| f9_prose | 0.944 | 0.079 | 4707 | 8716 | 0.94 | 1.00 | 1.00 | 1.00 | 0.94 | 0.83 | 1.00 | 1 | 18 |
| f2_grouped | 0.931 | 0.071 | 4226 | 7330 | 0.94 | 1.00 | 1.00 | 1.00 | 0.89 | 0.83 | 1.00 | 1 | 18 |

| format | resolve_owner | hub_load_yaml | override_collect | ambiguous_pin_model | lower_bound_registry_get | non_identifier_except |
|---|---|---|---|---|---|---|
| f1_lines | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| f4_xml | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| f5_json | 1.00 | 0.93 | 1.00 | 1.00 | 1.00 | 1.00 |
| f6_yaml | 1.00 | 1.00 | 1.00 | 1.00 | 0.89 | 1.00 |
| f7_edges | 1.00 | 1.00 | 0.83 | 1.00 | 1.00 | 1.00 |
| f3_tree | 1.00 | 1.00 | 0.75 | 1.00 | 1.00 | 1.00 |
| f8_mermaid | 1.00 | 1.00 | 0.75 | 1.00 | 0.95 | 1.00 |
| f9_prose | 1.00 | 1.00 | 1.00 | 0.67 | 1.00 | 1.00 |
| f2_grouped | 1.00 | 1.00 | 0.92 | 0.67 | 1.00 | 1.00 |

total cost $2.822 over 162 calls


### Paired comparison against the best format, error taxonomy, evidence

paired keys (case, rep): 18; best by mean accuracy: f1_lines (1.000)

| format | accuracy | diff vs best | 95% CI (bootstrap, paired) | cases where format is best/tied-best |
|---|---|---|---|---|
| f1_lines | 1.000 | +0.000 | [+0.000, +0.000] | 6/6 |
| f4_xml | 1.000 | +0.000 | [+0.000, +0.000] | 6/6 |
| f5_json | 0.989 | -0.011 | [-0.033, +0.000] | 5/6 |
| f6_yaml | 0.981 | -0.019 | [-0.056, +0.000] | 5/6 |
| f7_edges | 0.972 | -0.028 | [-0.069, +0.000] | 5/6 |
| f3_tree | 0.958 | -0.042 | [-0.083, +0.000] | 5/6 |
| f8_mermaid | 0.950 | -0.050 | [-0.097, -0.014] | 4/6 |
| f9_prose | 0.944 | -0.056 | [-0.167, +0.000] | 5/6 |
| f2_grouped | 0.931 | -0.069 | [-0.194, +0.000] | 4/6 |

| format | missed_items | invented_items | direction_flip | class_confusion | completeness_wrong | ambiguity_wrong | other |
|---|---|---|---|---|---|---|---|
| f1_lines | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| f4_xml | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| f5_json | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| f6_yaml | 0 | 0 | 0 | 0 | 0 | 0 | 1 |
| f7_edges | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| f3_tree | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| f8_mermaid | 1 | 0 | 0 | 0 | 0 | 0 | 3 |
| f9_prose | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| f2_grouped | 0 | 0 | 0 | 0 | 0 | 0 | 5 |

| format | header/footer-dependent Qs: accuracy | evidence cites header/footer |
|---|---|---|
| f1_lines | 1.000 | 92% |
| f4_xml | 1.000 | 72% |
| f5_json | 0.972 | 81% |
| f6_yaml | 0.972 | 36% |
| f7_edges | 0.944 | 92% |
| f3_tree | 0.917 | 92% |
| f8_mermaid | 0.917 | 92% |
| f9_prose | 0.972 | 89% |
| f2_grouped | 0.944 | 92% |

ceiling check: 6/9 formats ≥ 0.95 mean accuracy

| case/question | mean over all formats | worst formats (mean) |
|---|---|---|
| override_collect/Q4 | 0.67 | f3_tree 0.00, f8_mermaid 0.00, f7_edges 0.33 |
| ambiguous_pin_model/Q7 | 0.93 | f2_grouped 0.67, f9_prose 0.67, f1_lines 1.00 |
| ambiguous_pin_model/Q6a | 0.93 | f2_grouped 0.67, f9_prose 0.67, f1_lines 1.00 |
| ambiguous_pin_model/Q6b | 0.93 | f2_grouped 0.67, f9_prose 0.67, f1_lines 1.00 |
| ambiguous_pin_model/Q1 | 0.93 | f2_grouped 0.67, f9_prose 0.67, f1_lines 1.00 |
| hub_load_yaml/Q2 | 0.96 | f5_json 0.67, f1_lines 1.00, f2_grouped 1.00 |
| lower_bound_registry_get/Q4 | 0.96 | f6_yaml 0.67, f1_lines 1.00, f2_grouped 1.00 |
| lower_bound_registry_get/Q1 | 0.98 | f8_mermaid 0.84, f1_lines 1.00, f2_grouped 1.00 |
| resolve_owner/Q1 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| resolve_owner/Q2 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| resolve_owner/Q3 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| resolve_owner/Q4 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| resolve_owner/Q5 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| resolve_owner/Q5b | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| hub_load_yaml/Q1 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| hub_load_yaml/Q1b | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| hub_load_yaml/Q6 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| hub_load_yaml/Q7 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| override_collect/Q1 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| override_collect/Q2 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| override_collect/Q3 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| lower_bound_registry_get/Q2 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| non_identifier_except/Q8 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| non_identifier_except/Q9 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |
| non_identifier_except/Q10 | 1.00 | f1_lines 1.00, f2_grouped 1.00, f3_tree 1.00 |

## Interpretation

1. **There is a ceiling effect.** 6 of 9 formats score ≥ 0.95, and 17 of 25 questions scored 1.00 in every format. Sonnet reads an integrated, annotated match list correctly in nearly any encoding. Most of what separates the formats is aggregation (summing shown + footer counts) and output discipline, not reading relations.
2. **Lines and XML win.** Both scored 1.000 on all 6 cases and all 3 repeats (sd 0). They tie within the CI, and the tie-break is input tokens: **lines (format 1) 5,086 vs XML 5,816**. Lines is the only format that is both perfect and cheapest among the perfect ones.
3. **Only Mermaid is significantly worse** (CI excludes 0: [−0.097, −0.014]). It missed graph-only callers (MeetingRegistry Q1 0.53) and mis-summed the docs count. Every other difference has a CI touching 0 with n = 18 paired cells, so none of them is proven.
4. **The failures that did happen are not relation errors.** There were zero class confusions, zero direction flips and zero invented items. They were:
   - summing "shown + not shown" (override Q4: tree, Mermaid and edges answered 34 instead of 36);
   - two unparseable answers (grouped and prose on `_pin_model`: prose before the JSON, and malformed JSON);
   - two plausible misreadings of question scope: JSON read the hidden-dir omission as caller incompleteness, and YAML answered the repo-wide 1,766 ambiguous refs instead of the 91 found by the search.
5. **Grouped (`rg --heading`) is cheapest (4,226 tokens) but least stable** (sd 0.071, one unparseable answer). The failures don't point to the heading layout itself, but it didn't earn a tie either.

**Recommended top 4 for phase 2** (filter robustness, `| grep -v test` applied for real):
- **lines (1):** winner;
- **XML (4):** co-winner;
- **JSON (5):** third by accuracy, the natural machine envelope;
- **grouped (2):** cheapest, and the direct test of your `rg --heading` question. Phase 2 is exactly where its non-self-contained lines would break.

YAML (0.981) is left out because it costs the most tokens (6,293) and cited header/footer evidence least (36%).

**Ceiling note:** given the ceiling, phase 2 or harder questions are needed to separate the leaders. Harder questions would mean multi-hop across targets, or aggregation over footer and list. A smaller model would also help. Neither was run; that needs your approval.

## Product finding (outside the eval)

In the real search text, references for an ambiguous name are tagged `→ _pin_model`, the short name only. The header says they are "tagged with the one they resolve to", but the tag doesn't say which definition. The eval records took `resolves_to` from a per-candidate `graphite blast`. Graphite's own output should print the definition's `path:line` in that tag.

## Cost

Pilot (resolve_owner × 9, with the old Q3 wording; discarded) cost $0.311. Phase 1 (162 calls) cost $2.822. **Total: $3.13**, under the $5 cap.
