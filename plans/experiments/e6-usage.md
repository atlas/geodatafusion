# E6: usage

Run on 2026-10-05. Scripts are in [`experiments/e6-usage/`](../../experiments/e6-usage/). Raw API
responses are cached under `target/experiments/e6/` (not committed). The full ranked table is
[`experiments/e6-usage/usage.csv`](../../experiments/e6-usage/usage.csv).

## Summary

| Hypothesis | Verdict under the pre-registered rule |
|---|---|
| H15: the skip candidates are each in the bottom 10% by usage | **Rejected as stated.** 4 of 13 candidates are in the bottom 10%: ST_Letters, ST_GeomFromMARC21, ST_AsMARC21, ST_ForceSFS. The other 9 aren't: XML input (ST_GeomFromGML p24, ST_GeomFromKML p21, ST_GMLToSQL p11), ST_AsX3D p59, ST_MemSize p30, and the curve shims ST_CurveToLine p65, ST_HasArc p50, ST_LineToCurve p31, ST_ForceCurve p25. Under the rule, only ST_Letters, ST_GeomFromMARC21 and ST_ForceSFS may be skipped. |
| H3a: few downstream projects depend on what the breaking changes touch | **Holds.** No public project's code would stop compiling or fail its tests. 4 public projects call a touched function (ST_AsText/ST_AsBinary) and 2 of them would see different output. Both counts are under 5, so the rule says to batch the breaking changes into one release. |
| H8: the aggregate forms are used more than the scalar forms | **Holds for ST_Union (88% aggregate) and ST_Collect (82%), not for ST_MakeLine (27%).** Across all three, 1,290 of 2,013 classified calls (64%) are aggregate. Most usage is ST_Union and ST_Collect, so the rule says to prioritise the upstream DataFusion fallback over shipping `_agg` names. |

## Hypotheses (from [hypotheses.md](../hypotheses.md), unchanged)

- **H15 (D15, prioritisation).** The functions proposed for skipping (XML input, MARC21, X3D,
  `ST_Letters`, `ST_MemSize`, curve-function shims) are each in the bottom 10% of PostGIS
  functions by usage. **Rule:** skip a function only if it's in the bottom 10% and costs a new
  dependency or a shim. The usage ranking also orders work within each group's phases.
- **H3a (D3).** Few downstream projects depend on what the breaking changes touch. **Rule:**
  batch the breaking changes into one release if fewer than five public projects are affected.
  Otherwise deprecate first.
- **H8 (D8).** PostGIS's aggregate forms of `ST_Collect`, `ST_MakeLine` and `ST_Union` are used
  more than their scalar forms. **Rule:** if so, prioritise the upstream DataFusion fallback over
  shipping `_agg` names.

## H15: usage of every inventory function

### Method

**Functions.** All 332 rows of [inventory.md](../inventory.md), minus the 30 operators and 5 type
names, which code search can't count (it drops punctuation, and `geometry` is a common word).
That leaves 297 functions.

**Primary source: GitHub code search.** This is the REST `search/code` endpoint, which uses
GitHub's legacy index. It's the only code search the API offers. There's one query per function
and language:

```
"<name>" language:SQL
"<name>" language:PLpgSQL
```

GitHub's language detection splits `.sql` files between SQL and PLpgSQL (and others), so both
count as SQL code. Python was also fetched as a third, secondary language, but it's too noisy to
rank on. The phrase `"ST_Buffer"` matches 228k Python files, mostly variables like `st_buffer`.
Each query fetched page 1 (up to 100 files) with text-match fragments.

**False-positive correction.** The legacy index tokenises on `_` and is case-insensitive. So
`"ST_M"` matches `last_modify_time` (395k SQL files), `"GeometryType"` matches
`ST_GeometryType`, and `"ST_Snap"` matches files that use `ST_SnapToGrid`. Each sampled file is
put in one class, using the match offsets in its fragments (UTF-8 byte offsets):

- **use**: the exact name (case-insensitive) with no identifier character right before it (a
  `schema.` or `func.` prefix is allowed), followed by optional whitespace and `(`;
- **definition**: the name follows `FUNCTION`/`AGGREGATE`/`PROCEDURE` (`CREATE FUNCTION`,
  `COMMENT ON FUNCTION`, `create or replace function sedona.ST_X`). This covers other engines'
  DDL (MonetDB, Sedona, Esri Hive) and schema dumps;
- **vendored**: a copy of PostGIS's own sources or regression suite inside another repository
  (paths with `regress/`, `postgis-<version>/`, `postgis_source/`, `contrib/postgis/`,
  `postgis--x.y.sql`, `legacy.sql`, openGauss `postgis_function*.sql`), or a Rails
  `structure.sql` dump;
- **noise**: anything else, such as prose, identifiers containing the name, or a different
  function.

`precision = use / sampled`. The per-language estimate is `total_count × precision`, or the
exact `use` count when every result was sampled (total ≤ 100). **Score** is the SQL estimate
plus the PLpgSQL estimate, an estimate of the number of files in the index that call the
function. **Percentile** is mid-rank: the percentage of the other functions with a lower score,
counting ties as half. "Bottom 10%" means percentile < 10. A function scoring 0 is at p3.4,
because 21 functions tie at 0.

**Sensitivity metric (repo-adjusted).** One repository can contribute dozens of files: 95 of the
100 sampled ST_AsX3D SQL files come from 4 repositories. The repo-adjusted score scales each
estimate by `distinct repositories / use files` in the sample.

**Secondary source: GIS Stack Exchange.** This used `/2.3/search/excerpts?site=gis&q=<name>`
(questions and answers), reading `total`. The quota without a key is about 300 requests a day,
so it covered a fixed sample: every second function in inventory order plus all 13 skip
candidates, 153 functions in all. This took 153 requests plus about 10 to test the endpoint;
`/search/advanced` was rejected because it only searches questions.

**Deep-page check.** For 19 noisy or high-count names, page 5 (results 401–500) was also
fetched, to see whether page 1's precision holds deeper in the result list.

**Rate limits.** Code search allows 10 requests a minute. Requests were spaced 6.5 s apart. On a
403 or 429 the script waited for `Retry-After`, or for `X-RateLimit-Reset` when the remaining
quota was 0, and otherwise backed off. The primary SQL/PLpgSQL pass (594 requests, 2026-10-05
15:38–17:08 UTC) hit one 403. The H3a, H8 and deep-page queries ran after it the same day. The
Python pass ran last and started getting repeated 429s ("try again in 220 s") after about 160
functions, so it was stopped at 164 of 297. Python is secondary and isn't used in the score.

### Validation

| Check | Result |
|---|---|
| Spearman(score, SE posts), n = 153 | ρ = 0.905 |
| Spearman(repo-adjusted score, SE posts), n = 153 | ρ = 0.917 |
| Spearman(SQL estimate, PLpgSQL estimate), n = 297 | ρ = 0.894 |
| Spearman(score, repo-adjusted score), n = 297 | ρ = 0.985 |
| Spearman(score, raw total_count), n = 297 | ρ = 0.927 |
| Spearman(score, Python estimate), n = 164 (Python fetch stopped early, see Method) | ρ = 0.771 |
| Spearman(Python estimate, SE posts), n = 84 | ρ = 0.742 |
| Bottom-15 overlap, GitHub vs SE (within the SE sample) | 8 of 15 |

The GitHub ranking and the Stack Exchange ranking agree closely (ρ ≈ 0.9). The bottom decile is
noisier, because most functions there have zero to two uses in both sources.

Deep-page precision for SQL, page 1 vs page 5:

| Name | SQL total | Page 1 | Page 5 |
|---|---|---|---|
| ST_X | 8528 | 0.94 | 0.36 |
| ST_Y | 19776 | 0.97 | 0.14 |
| ST_Z | 4296 | 0.46 | 0.02 |
| ST_M | 395264 | 0.00 | 0.00 |
| ST_Point | 4576 | 0.78 | 0.22 |
| ST_Union | 4560 | 0.93 | 0.53 |
| GeometryType | 2424 | 0.47 | 0.32 |
| ST_Area | 4840 | 0.93 | 0.48 |
| ST_Length | 2544 | 0.96 | 0.50 |
| ST_Intersects | 5184 | 0.96 | 0.93 |
| Box2D | 521 | 0.20 | 0.16 |
| ST_Collect | 3960 | 0.95 | 0.10 |
| ST_Transform | 4544 | 0.98 | 0.64 |
| ST_Buffer | 2136 | 0.90 | 0.65 |
| ST_Snap | 7696 | 0.11 | 0.01 |
| ST_Project | 3512 | 0.19 | 0.01 |
| ST_Points | 1330 | 0.11 | 0.01 |

Results are ordered by relevance, so precision falls deeper in the list. Extrapolating page 1's
precision therefore **overstates high-count functions**, by up to about 5–10× for names that
collide with other tokens (ST_Collect/ST_CollectionExtract, ST_Snap/ST_SnapToGrid, ST_Y). This
compresses the top of the ranking but barely touches the bottom decile. Every skip candidate has
a raw total of at most 303 (most are at or under about 120), so its sample covers a third to
all of its results, and its estimate is close to an exact count. Correcting the inflated
functions above a candidate can only raise that candidate's percentile.

Rows marked "low" in the CSV extrapolate more than 2× beyond the sample from a weak precision
estimate (fewer than 10 uses, or precision below 0.3). These are ST_Snap, ST_Project, ST_M,
ST_Rotate, ST_Node, ST_AsEWKT, ST_Summary, ST_Relate, ST_Polygon, Box2D, ST_Points, ST_IsSimple,
ST_SymDifference, ST_Scale, ST_ForceRHR, ST_AsGML, ST_Normalize, Box3D, ST_ContainsProperly and
ST_RelateMatch. Stack Exchange confirms ST_Snap (119 posts, against 1,162 for ST_Buffer) and
ST_Project (57) are overstated.

### Skip candidates

| Function | Rank | Score | Percentile | Bottom 10% | Repo-adj. score | Repo pct | SQL raw / prec. / repos in sample | PLpgSQL raw / prec. / repos | SE posts (pct in SE sample) |
|---|---|---|---|---|---|---|---|---|---|
| ST_CurveToLine | 104/297 | 272 | 65.2 | no | 114 | 61.5 | 303 / 0.60 / 19 | 181 / 0.50 / 31 | 38 (66) |
| ST_AsX3D | 122/297 | 177 | 59.1 | no | 11 | 29.4 | 182 / 0.95 / 4 | 102 / 0.04 / 4 | 5 (37) |
| ST_HasArc | 149/297 | 71 | 50.0 | no | 68 | 56.4 | 119 / 0.55 / 53 | 81 / 0.07 / 5 | 3 (30) |
| ST_LineToCurve | 204/297 | 19 | 31.4 | no | 19 | 38.5 | 161 / 0.09 / 9 | 108 / 0.04 / 4 | 8 (41) |
| ST_MemSize | 207/297 | 18 | 30.4 | no | 17 | 36.5 | 107 / 0.11 / 10 | 82 / 0.07 / 6 | 1 (16) |
| ST_ForceCurve | 223/297 | 14 | 25.0 | no | 10 | 26.9 | 41 / 0.20 / 6 | 39 / 0.15 / 4 | 1 (16) |
| ST_GeomFromGML | 226/297 | 13 | 24.0 | no | 10 | 28.4 | 119 / 0.08 / 6 | 107 / 0.03 / 3 | 17 (56) |
| ST_GeomFromKML | 235/297 | 9 | 20.9 | no | 9 | 24.3 | 113 / 0.04 / 4 | 92 / 0.04 / 4 | 13 (52) |
| ST_GMLToSQL | 264/297 | 2 | 11.3 | no | 2 | 12.2 | 81 / 0.00 / 0 | 76 / 0.03 / 2 | 1 (16) |
| ST_ForceSFS | 270/297 | 1 | 8.3 | yes | 1 | 8.6 | 53 / 0.00 / 0 | 49 / 0.02 / 1 | 0 (5) |
| ST_GeomFromMARC21 | 287/297 | 0 | 3.4 | yes | 0 | 3.4 | 17 / 0.00 / 0 | 28 / 0.00 / 0 | 0 (5) |
| ST_AsMARC21 | 288/297 | 0 | 3.4 | yes | 0 | 3.4 | 10 / 0.00 / 0 | 28 / 0.00 / 0 | 0 (5) |
| ST_Letters | 297/297 | 0 | 3.4 | yes | 0 | 3.4 | 63 / 0.00 / 0 | 71 / 0.00 / 0 | 0 (5) |

The bottom-10% boundary is a score of about 1.2: 30 functions score 1 or less.

#### Verdict per candidate (rule: bottom 10% and costs a new dependency or a shim)

| Function | Bottom 10% | Cost (from the group plans) | Under the rule |
|---|---|---|---|
| ST_Letters | yes | GPL-2.0 font geometry data (a new, licence-incompatible data dependency) | may skip |
| ST_GeomFromMARC21 | yes | XML parser (`quick-xml`) | may skip |
| ST_ForceSFS | yes | shim | may skip |
| ST_AsMARC21 | yes | none: MARCXML can be written as a string | the rule doesn't license skipping; usage is zero, so it goes last |
| ST_GMLToSQL | no (p11.3, just above) | XML parser; it's an alias of ST_GeomFromGML | don't skip; free once ST_GeomFromGML exists |
| ST_GeomFromGML, ST_GeomFromKML | no (p24, p21; SE p56, p52) | XML parser | don't skip; late phase, behind a feature if wanted |
| ST_AsX3D | no (p59; p29 repo-adjusted) | none | don't skip; late phase (as G4 already orders it) |
| ST_MemSize | no (p30) | shim (no meaningful Arrow equivalent) | the rule doesn't license skipping |
| ST_HasArc, ST_CurveToLine, ST_LineToCurve, ST_ForceCurve | no (p50, p65, p31, p25) | shims | the rule doesn't license skipping, so add the shims |

**H15 as stated ("each in the bottom 10%") is rejected:** 4 of 13 candidates are in the bottom
10%. Excluding regression-suite copies matters a lot here. Before that filter, the XML and
MemSize candidates scored 3–5× higher. With or without it, none of them reaches the bottom 10%.

**On the rule.** The bottom 10% is almost all functions with zero to two uses in the index, many
of them added in PostGIS 3.2–3.6 (ST_PointM/ST_PointZM, the FlatGeobuf functions,
`postgis_srs*`, ST_CoverageUnion/ST_CoverageClean, ST_Square, ST_LargestEmptyCircle). Public code
lags new releases, so a bottom-10% cut also selects "new", not only "unwanted". The cut is also
strict: a function used in a handful of repositories clears it. The four curve shims clear it
partly because of a Polish university course: dozens of student repositories run the same
ST_HasArc/ST_CurveToLine exercise. These are judgement points for the maintainer. The rule is
applied as written above.

### Ranked usage (top 40 and bottom 35; all 297 in `usage.csv`)

"Pct" is the file-based percentile and "Repo pct" the repo-adjusted one. "Conf." marks
low-confidence extrapolation.

| Rank | Function | Group | Impl. | Score | Pct | Repo pct | SQL raw / prec. | PLpgSQL raw / prec. | SE posts | Conf. |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | ST_Y | G1 | yes | 26248 | 100.0 | 100.0 | 19776 / 0.97 | 7680 / 0.92 | 454 |  |
| 2 | ST_SetSRID | G6 |  | 17411 | 99.7 | 99.7 | 6496 / 0.96 | 11520 / 0.97 | 1282 |  |
| 3 | ST_MakePoint | G1 | yes | 15063 | 99.3 | 99.3 | 5776 / 0.97 | 10064 / 0.94 |  |  |
| 4 | ST_X | G1 | yes | 14743 | 99.0 | 99.0 | 8528 / 0.94 | 7312 / 0.92 |  |  |
| 5 | ST_Distance | G2 | yes | 10848 | 98.6 | 98.6 | 4208 / 0.85 | 7904 / 0.92 | 1185 |  |
| 6 | ST_DWithin | G2 |  | 9023 | 98.3 | 98.3 | 3056 / 0.92 | 6608 / 0.94 |  |  |
| 7 | ST_Intersects | G2 | yes | 8709 | 98.0 | 97.6 | 5184 / 0.96 | 3888 / 0.96 |  |  |
| 8 | ST_Transform | G3 |  | 7620 | 97.6 | 98.0 | 4544 / 0.98 | 3232 / 0.98 | 1846 |  |
| 9 | ST_Collect | G1+G5 |  | 7072 | 97.3 | 96.6 | 3960 / 0.95 | 3448 / 0.96 |  |  |
| 10 | ST_Area | G2 | yes | 6819 | 97.0 | 97.3 | 4840 / 0.93 | 2440 / 0.95 | 687 |  |
| 11 | ST_Multi | G1 |  | 6093 | 96.6 | 94.6 | 3360 / 0.97 | 3080 / 0.92 |  |  |
| 12 | ST_Union | G3+G5 |  | 5909 | 96.3 | 95.6 | 4560 / 0.93 | 2060 / 0.81 | 1412 |  |
| 13 | ST_Point | G1 | yes | 5490 | 95.9 | 97.0 | 4576 / 0.78 | 3920 / 0.49 | 475 |  |
| 14 | ST_GeomFromText | G4 | yes | 5145 | 95.6 | 95.9 | 4592 / 0.82 | 1452 / 0.95 |  |  |
| 15 | ST_Contains | G2 | yes | 4284 | 95.3 | 96.3 | 2584 / 0.92 | 2192 / 0.87 |  |  |
| 16 | ST_Length | G2 | yes | 4118 | 94.9 | 92.9 | 2544 / 0.96 | 1728 / 0.97 |  |  |
| 17 | ST_Centroid | G2 | yes | 4078 | 94.6 | 93.2 | 2448 / 0.97 | 1832 / 0.93 | 509 |  |
| 18 | ST_AsText | G4 | yes | 3789 | 94.3 | 94.9 | 2920 / 0.67 | 1992 / 0.92 |  |  |
| 19 | ST_Dump | G5 | yes | 3567 | 93.9 | 93.9 | 1920 / 0.93 | 1916 / 0.93 |  |  |
| 20 | ST_Intersection | G3 |  | 3389 | 93.6 | 92.6 | 2036 / 0.93 | 1608 / 0.93 |  |  |
| 21 | ST_Buffer | G3 |  | 3304 | 93.2 | 93.6 | 2136 / 0.9 | 1552 / 0.89 | 1162 |  |
| 22 | ST_AsGeoJSON | G4 |  | 3301 | 92.9 | 90.9 | 1748 / 0.94 | 1692 / 0.98 | 380 |  |
| 23 | ST_Z | G1 | yes | 2862 | 92.6 | 95.3 | 4296 / 0.46 | 1230 / 0.72 |  |  |
| 24 | ST_EndPoint | G1 | yes | 2634 | 92.2 | 90.2 | 1116 / 0.91 | 1668 / 0.97 | 251 |  |
| 25 | ST_GeometryType | G1 | yes | 2572 | 91.9 | 90.5 | 1476 / 0.82 | 1404 / 0.97 | 121 |  |
| 26 | ST_MakeLine | G1+G5 |  | 2515 | 91.6 | 91.6 | 1104 / 0.91 | 1660 / 0.91 | 613 |  |
| 27 | ST_Snap | G3 |  | 2494 | 91.2 | 94.3 | 7696 / 0.11 | 4336 / 0.38 | 119 | low |
| 28 | ST_Within | G2 | yes | 2461 | 90.9 | 92.2 | 1704 / 0.91 | 1034 / 0.88 | 601 |  |
| 29 | ST_IsValid | G3 | yes | 2380 | 90.5 | 91.9 | 1520 / 0.84 | 1226 / 0.9 | 218 |  |
| 30 | ST_SRID | G6 |  | 2377 | 90.2 | 91.2 | 1164 / 0.73 | 1660 / 0.92 |  |  |
| 31 | ST_StartPoint | G1 | yes | 2150 | 89.9 | 88.5 | 836 / 0.9 | 1456 / 0.96 |  |  |
| 32 | GeometryType | G1 | yes | 2123 | 89.5 | 89.2 | 2424 / 0.47 | 2460 / 0.4 | 576 |  |
| 33 | ST_MakeEnvelope | G1 |  | 2121 | 89.2 | 86.5 | 964 / 0.89 | 1316 / 0.96 | 242 |  |
| 34 | ST_GeomFromGeoJSON | G4 |  | 2060 | 88.9 | 77.4 | 802 / 0.88 | 1368 / 0.99 |  |  |
| 35 | ST_Project | G1 |  | 1794 | 88.5 | 89.9 | 3512 / 0.19 | 1976 / 0.57 | 57 | low |
| 36 | ST_M | G1 | yes | 1725 | 88.2 | 88.9 | 395264 / 0.0 | 86272 / 0.02 | 21 | low |
| 37 | ST_Envelope | G1 |  | 1680 | 87.8 | 88.2 | 930 / 0.84 | 936 / 0.96 |  |  |
| 38 | ST_Equals | G2 | yes | 1669 | 87.5 | 85.8 | 850 / 0.56 | 1356 / 0.88 | 189 |  |
| 39 | ST_IsEmpty | G1 | yes | 1667 | 87.2 | 87.5 | 740 / 0.85 | 1180 / 0.88 |  |  |
| 40 | ST_MakeValid | G3 |  | 1615 | 86.8 | 87.2 | 766 / 0.95 | 944 / 0.94 |  |  |
| … | | | | | | | | | | |
| 263 | ST_SimplifyPolygonHull | G3 |  | 2 | 11.3 | 12.2 | 16 / 0.12 | 29 / 0.0 |  |  |
| 264 | ST_GMLToSQL | G4 |  | 2 | 11.3 | 12.2 | 81 / 0.0 | 76 / 0.03 | 1 |  |
| 265 | ST_ClusterWithinWin | G5 |  | 2 | 11.3 | 12.2 | 9 / 0.0 | 13 / 0.15 | 3 |  |
| 266 | ST_CoverageClean | G5 |  | 2 | 11.3 | 8.6 | 2 / 1.0 | 2 / 0.0 |  |  |
| 267 | ST_PatchN | G1 |  | 1 | 10.1 | 10.8 | 146 / 0.01 | 57 / 0.0 | 0 |  |
| 268 | ST_NumPatches | G1 |  | 1 | 9.8 | 10.5 | 125 / 0.01 | 57 / 0.0 | 0 |  |
| 269 | ST_CurveN | G1 |  | 1 | 8.3 | 8.6 | 8 / 0.0 | 12 / 0.08 | 0 |  |
| 270 | ST_ForceSFS | G1 |  | 1 | 8.3 | 8.6 | 53 / 0.0 | 49 / 0.02 | 0 |  |
| 271 | ST_RemoveSmallParts | G1 |  | 1 | 8.3 | 8.6 | 9 / 0.11 | 10 / 0.0 |  |  |
| 272 | ST_ClosestPointOfApproach | G1 |  | 1 | 8.3 | 8.6 | 34 / 0.0 | 34 / 0.03 |  |  |
| 273 | ST_IsValidTrajectory | G1 |  | 1 | 8.3 | 8.6 | 34 / 0.0 | 34 / 0.03 | 1 |  |
| 274 | ST_MinimumClearance | G3 |  | 1 | 8.3 | 8.6 | 34 / 0.03 | 33 / 0.0 | 2 |  |
| 275 | ST_MinimumClearanceLine | G3 |  | 1 | 8.3 | 8.6 | 33 / 0.03 | 33 / 0.0 |  |  |
| 276 | ST_TriangulatePolygon | G3 |  | 1 | 8.3 | 8.6 | 14 / 0.0 | 28 / 0.04 |  |  |
| 277 | ST_NumCurves | G1 |  | 0 | 3.4 | 3.4 | 8 / 0.0 | 11 / 0.0 | 0 |  |
| 278 | ST_PointM | G1 | yes | 0 | 3.4 | 3.4 | 16 / 0.0 | 29 / 0.0 |  |  |
| 279 | ST_PointZM | G1 | yes | 0 | 3.4 | 3.4 | 17 / 0.0 | 29 / 0.0 |  |  |
| 280 | ST_Square | G1 |  | 0 | 3.4 | 3.4 | 135 / 0.0 | 290 / 0.0 |  |  |
| 281 | ST_Scroll | G1 |  | 0 | 3.4 | 3.4 | 140 / 0.0 | 49 / 0.0 |  |  |
| 282 | ST_FilterByM | G1 |  | 0 | 3.4 | 3.4 | 28 / 0.0 | 33 / 0.0 |  |  |
| 283 | ST_PointInsideCircle | G1 |  | 0 | 3.4 | 3.4 | 12 / 0.0 | 37 / 0.0 | 3 |  |
| 284 | ST_CPAWithin | G1 |  | 0 | 3.4 | 3.4 | 34 / 0.0 | 33 / 0.0 | 1 |  |
| 285 | ST_LargestEmptyCircle | G3 |  | 0 | 3.4 | 3.4 | 9 / 0.0 | 11 / 0.0 | 0 |  |
| 286 | ST_InverseTransformPipeline | G3 |  | 0 | 3.4 | 3.4 | 9 / 0.0 | 11 / 0.0 |  |  |
| 287 | ST_GeomFromMARC21 | G4 |  | 0 | 3.4 | 3.4 | 17 / 0.0 | 28 / 0.0 | 0 |  |
| 288 | ST_AsMARC21 | G4 |  | 0 | 3.4 | 3.4 | 10 / 0.0 | 28 / 0.0 | 0 |  |
| 289 | ST_CoverageUnion | G5 |  | 0 | 3.4 | 3.4 | 10 / 0.0 | 11 / 0.0 | 1 |  |
| 290 | ST_FromFlatGeobuf | G5 |  | 0 | 3.4 | 3.4 | 21 / 0.0 | 29 / 0.0 |  |  |
| 291 | ST_FromFlatGeobufToTable | G5 |  | 0 | 3.4 | 3.4 | 21 / 0.0 | 29 / 0.0 | 0 |  |
| 292 | ST_AsFlatGeobuf | G5 |  | 0 | 3.4 | 3.4 | 30 / 0.0 | 27 / 0.0 |  |  |
| 293 | postgis_srs | G5 |  | 0 | 3.4 | 3.4 | 4 / 0.0 | 11 / 0.0 | 0 |  |
| 294 | postgis_srs_all | G5 |  | 0 | 3.4 | 3.4 | 4 / 0.0 | 11 / 0.0 |  |  |
| 295 | postgis_srs_codes | G5 |  | 0 | 3.4 | 3.4 | 4 / 0.0 | 11 / 0.0 | 0 |  |
| 296 | postgis_srs_search | G5 |  | 0 | 3.4 | 3.4 | 4 / 0.0 | 11 / 0.0 |  |  |
| 297 | ST_Letters | — |  | 0 | 3.4 | 3.4 | 63 / 0.0 | 71 / 0.0 | 0 |  |

The 30 operators (`&&`, `<->`, `~=`, …) and the type names aren't ranked. Code search can't
find them.

### Highest-usage unimplemented functions (for phasing)

Top 15 overall (score, percentile). ST_Snap and ST_Project are low confidence and probably
overstated; their Stack Exchange counts are 119 and 57.

| Rank | Function | Group | Score | Pct |
|---|---|---|---|---|
| 2 | ST_SetSRID | G6 | 17411 | 99.7 |
| 6 | ST_DWithin | G2 | 9023 | 98.3 |
| 8 | ST_Transform | G3 | 7620 | 97.6 |
| 9 | ST_Collect | G1+G5 | 7072 | 97.3 |
| 11 | ST_Multi | G1 | 6093 | 96.6 |
| 12 | ST_Union | G3+G5 | 5909 | 96.3 |
| 20 | ST_Intersection | G3 | 3389 | 93.6 |
| 21 | ST_Buffer | G3 | 3304 | 93.2 |
| 22 | ST_AsGeoJSON | G4 | 3301 | 92.9 |
| 26 | ST_MakeLine | G1+G5 | 2515 | 91.6 |
| 27 | ST_Snap (low) | G3 | 2494 | 91.2 |
| 30 | ST_SRID | G6 | 2377 | 90.2 |
| 33 | ST_MakeEnvelope | G1 | 2121 | 89.2 |
| 34 | ST_GeomFromGeoJSON | G4 | 2060 | 88.9 |
| 35 | ST_Project (low) | G1 | 1794 | 88.5 |

Top unimplemented per group (score, percentile):

- **G1:** ST_Multi (6093, p97), ST_MakeEnvelope (2121, p89), ST_Project (1794, p89, low),
  ST_Envelope (1680, p88), ST_CollectionExtract (1546, p86), ST_MakePolygon (1102, p83),
  ST_Translate (966, p82), ST_PointN (960, p82).
- **G1+G5:** ST_Collect (7072, p97), ST_MakeLine (2515, p92).
- **G2:** ST_DWithin (9023, p98), ST_LineLocatePoint (1094, p83), ST_Azimuth (880, p79),
  ST_Perimeter (625, p74), ST_DistanceSphere (534, p72), ST_Relate (287, p67, low).
- **G3:** ST_Transform (7620, p98), ST_Intersection (3389, p94), ST_Buffer (3304, p93),
  ST_Snap (2494, p91, low), ST_MakeValid (1615, p87), ST_Difference (1409, p85), ST_Split (923,
  p80), ST_AsMVTGeom (804, p77).
- **G3+G5:** ST_Union (5909, p96), ST_Polygonize (272, p65).
- **G4:** ST_AsGeoJSON (3301, p93), ST_GeomFromGeoJSON (2060, p89), ST_GeogFromText (1058,
  p82), ST_AsEWKT (462, p72), ST_GeomFromEWKT (445, p71), ST_PointFromText (394, p70),
  ST_AsEWKB (232, p63).
- **G5:** ST_DumpPoints (926, p81), ST_AsMVT (807, p78), ST_DumpRings (385, p70),
  ST_ClusterDBSCAN (360, p69), ST_Subdivide (272, p65).
- **G6:** ST_SetSRID (17411, p100), ST_SRID (2377, p90). The `&&` and `<->` operators can't be
  measured here.

Implications for phasing:

- **ST_SetSRID is the most-used unimplemented function** (second overall) and is already in
  phase 1.
- **ST_DWithin (6th) and ST_Transform (8th) are the next two.** ST_DWithin is a G2 function
  with no new dependency, so it's worth pulling forward.
- **ST_Collect, ST_Union and ST_MakeLine are all in the top 26.** That raises the stakes for
  D8 (see H8).
- **ST_Multi (11th) and ST_AsGeoJSON / ST_GeomFromGeoJSON** are cheap native functions with
  high usage.

## H3a: downstream users and the breaking changes

### Method

- **crates.io.** Data came from `GET /api/v1/crates/geodatafusion` and
  `/reverse_dependencies` (with a User-Agent naming the experiment). The source of each
  dependent crate's latest version was downloaded from static.crates.io and grepped. arrow-pg
  0.15.0 was also downloaded, because datafusion-postgres renders geodatafusion output through
  it.
- **PyPI.** pypistats.org `/recent` and `/overall`.
- **GitHub dependency graph.** The "Used by" pages were scraped (`dependents.sh`): 105
  repositories, of which 65 are listed. There was one code search per listed repository for
  `geodatafusion repo:<r>`, to separate direct use from Cargo.lock-only (transitive) entries.
- **GitHub code search** for `geodatafusion` in Rust, Python, TOML, `Cargo.toml`,
  `pyproject.toml`, `requirements.txt`, Markdown and notebooks, all pages. The candidate
  repositories were shallow-cloned and grepped for every touched API:
  ST_NPoints/ST_NumInteriorRings/ST_CoordDim/ST_NDims (UInt return types), GeometryType and
  ST_GeometryType (Utf8View), ST_AsText/ST_AsBinary (tagged output), GeoHash names and the
  module paths (`udf::geohash`, `geodatafusion.geohash`, `register_all_geohash`,
  `Box2DFromGeoHash`), and `features = ["geos"]` on the dependency.

### Data

- **crates.io:** 1,128,902 total downloads and 526,063 in the last 90 days. 7 reverse
  dependencies:

  | Crate | Downloads | Uses |
  |---|---|---|
  | lance-geo 12.0.0 | 769,722 | `geodatafusion::register` plus explicit Area/Distance/Length/relationship/IsValid UDFs |
  | datafusion-postgres 0.18.0 | 67,121 | `geodatafusion::register` behind its `postgis` feature (now on a git fork for DataFusion 55) |
  | zebflow 0.11.1 | 361 | `register`; a test uses `ST_AsText` |
  | fv-plan 0.2.0 | 109 | `register`; a test reads `ST_AsText` output with `as_string::<i32>()` |
  | geodatafusion-flatgeobuf / -geojson / -geoparquet 0.5.0 | 339 / 586 / 692 | Same repository (sibling crates) |

  Almost all of the download volume comes through lance (lance-geo is an optional `geo` feature).
- **PyPI:** 135 downloads in the last month and 36 in the last week. Since 2026-04-07 there have
  been 2,415 downloads without mirrors.
- **GitHub dependency graph:** 65 listed repositories. Only 4 of them mention geodatafusion
  outside a Cargo.lock: datafusion-postgres, lance-format/lance, its fork exa-labs/lance, and
  Rheosoph/flow-like. The other 61 have it only in a Cargo.lock, pulled in through lance or
  lancedb (ceph, cocoindex, milvus-storage, rerun, …), or had no hit in the index. The package
  view lists 4: datafusion-postgres, geodatafusion, lance and zebflow.
- **Public projects with direct use** (forks and the geodatafusion repository itself left out;
  2 private repositories that showed up for the authenticated user also left out):
  - Rust: lance (lance-geo), datafusion-postgres, zebflow, fv-plan, Rheosoph/flow-like,
    agnosticeng/geolookup, developmentseed/zarr-datafusion-search, ddupg/lance-geo-test,
    notreallystatic/bug-replicate.
  - Python: developmentseed/lazymerge, zarr-datafusion-search (Python bindings and examples
    repository), hyperdimensionalcomputingai/telecom-incident-triage,
    kylebarron/datafusion-nyc-2025-demo, richban/opendata-stack-platform.

  That's 14 projects. Embucket has a copy of old geodatafusion code, but no dependency on it.

### Affected projects per breaking change

| Change | Public projects whose code uses it | Effect |
|---|---|---|
| UInt → Int return types (ST_NPoints, ST_NumInteriorRings, ST_CoordDim, ST_NDims) | 0 in code | flow-like lists these in its SQL editor's autocomplete, and datafusion-postgres serves them over the wire (UInt32 → `int8` today, Int32 → `int4` after, matching PostGIS). No code breaks. |
| GeometryType/ST_GeometryType Utf8View → Utf8 | 0 | — |
| ST_AsText/ST_AsBinary without the GeoArrow tag | 4: flow-like, datafusion-postgres, zebflow, fv-plan | **flow-like** wraps ST_AsText to *strip* the tag ("geodatafusion 0.4 tags the WKT … so result readers would decode it back into GeoJSON"). The change makes its wrapper a no-op. **datafusion-postgres/arrow-pg** maps a tagged `geoarrow.wkb` result to pg type TEXT; untagged Binary becomes `bytea` (PostGIS's type), so ST_AsBinary's wire type changes. Its integration test only asserts non-NULL. **zebflow** and **fv-plan** tests read the text as `Utf8`, which still works. |
| GeoHash moved and renamed (`udf::geohash` → `native/io`, `geodatafusion.geohash` → `geodatafusion.native`, `Box2DFromGeoHash` → `Box2dFromGeoHash`) | 0 | flow-like lists the SQL names, which don't change. |
| Implicit `geos` feature removed | 0 | No dependent enables `features = ["geos"]`. |

**Verdict:** fewer than five public projects are affected. On the broadest reading ("uses a
touched function in code or tests") the count is 4. On the narrowest ("would fail to compile or
fail its tests") it's 0. Only 2 would see different output (flow-like, beneficially;
datafusion-postgres, toward PostGIS). **Rule: batch the breaking changes into one release.**

One caveat. lance-geo and datafusion-postgres call `geodatafusion::register`, so their own
users can run any function in SQL. That reach can't be measured from public code. Users of
lance's filter expressions that compare `ST_NPoints(...)` to an integer literal are unaffected
either way, because DataFusion coerces the literal. A release note for the datafusion-postgres
maintainer (ST_AsBinary becoming `bytea`) and for lance is cheap.

## H8: aggregate vs scalar forms

### Method

- **Sample:** code search `"<name>" language:SQL` pages 1–2, `language:PLpgSQL` page 1 and
  `language:Python` page 1, for ST_Collect, ST_MakeLine and ST_Union. That's up to 400 files
  per function, in best-match order. Each file was downloaded at the indexed commit
  (raw.githubusercontent.com). Files with identical content and vendored PostGIS copies were
  dropped (same rule as H15).
- **Parsing:** every `name(` occurrence (case-insensitive, not part of a longer identifier) is
  parsed with balanced parentheses and brackets, respecting quotes, and its top-level arguments
  are split. SQL `--` comments are stripped first. Definitions (`CREATE FUNCTION/AGGREGATE
  name`) are skipped.
- **Classes:**
  - *aggregate*: one argument that isn't an array; any call followed by `OVER` (window use) or
    with `ORDER BY` in its arguments; or ST_Union(g, numeric literal), the gridSize aggregate.
  - *scalar-array*: one argument that is `ARRAY[...]`, `ARRAY(...)`, `array_agg(...)`, a
    BigQuery-style `[...]` literal, or a `::geometry[]` cast.
  - *scalar*: two or more geometry arguments. ST_Union(g1, g2, gridSize) counts as scalar.
  - *ambiguous*: one bare variable in a PL/pgSQL assignment (`x := ST_MakeLine(pts)`), usually a
    `geometry[]` variable. These are left out of the share.
- **Spot check:** 36 random classified calls (12 per function) were checked by eye. 34 were
  right. The 2 wrong ones were PL/pgSQL array variables counted as ST_MakeLine aggregates, which
  led to the ambiguous class.

### Results

| Function | Files parsed | Aggregate calls | Scalar (2+ args) | Scalar (array) | Ambiguous | Other | Aggregate share | Files: agg-only / scalar-only / both / no call | Excluded (vendored / duplicate) |
|---|---|---|---|---|---|---|---|---|---|
| ST_Collect | 378 | 352 | 66 | 12 | 2 | 0 | 82% | 181 / 18 / 13 / 166 | 14 / 8 |
| ST_MakeLine | 340 | 199 | 438 | 102 | 9 | 5 | 27% | 117 / 185 / 15 / 23 | 24 / 36 |
| ST_Union | 342 | 739 | 97 | 8 | 1 | 0 | 88% | 260 / 31 / 12 / 39 | 24 / 34 |

"No call" files are search false positives, mostly ST_CollectionExtract for ST_Collect. The
aggregate share is `aggregate / (aggregate + scalar + scalar-array)`. The per-file view agrees:
for ST_Collect, 181 files use only the aggregate and 18 only the scalar form; for ST_Union, 260
against 31; for ST_MakeLine, 117 against 185.

Weighted by usage, ST_Union (rank 12) and ST_Collect (rank 9) outweigh ST_MakeLine (rank 26).
Across all three, 1,290 of 2,013 classified calls (64%) are aggregate.

**Verdict:** H8 holds for ST_Collect and ST_Union and fails for ST_MakeLine, where the
two-point scalar form dominates. Under the rule, the interim `_agg` naming would hurt most users
of ST_Union and ST_Collect, so **prioritise the upstream DataFusion fallback over shipping
`_agg` names**. The rule doesn't say what to do with a mixed result. A reasonable reading is that
ST_MakeLine's scalar form keeps the plain name in any interim scheme, and only its aggregate
needs the fallback.

## Threats to validity

- **The legacy code search index.** The REST API searches GitHub's legacy index: default
  branches only, files under 384 KB, forks excluded, and a subset of repositories. Results were
  current (files from 2025–2026 appear), but coverage is unknown. `total_count` is approximate.
- **Page-1 precision extrapolation.** Results are ordered by relevance, so precision falls
  deeper in the list (deep-page table). High-count functions are overstated, unevenly. The
  bottom decile and the skip candidates have near-complete samples, so they aren't affected
  much. The ranking above about p80 is coarse.
- **Classification errors.** The vendored and definition rules are path and regex heuristics.
  They changed the skip candidates' scores by 3–5×, but not their verdicts. Prose and comment
  mentions count as noise. Dynamically built SQL in other languages is missed.
- **Concentration.** Course assignments (dozens of student repositories with the same exercise)
  and single repositories with generated files inflate some functions. The repo-adjusted score
  handles the second case, not the first.
- **Dialect mixing.** BigQuery, Snowflake, Sedona and CartoDB use the same `ST_` names.
  Definitions are excluded, but calls in those dialects count as uses. For geodatafusion,
  which serves users of several engines, that's arguably what should be measured.
- **Recency bias.** Functions added in PostGIS 3.2–3.6 score low in public code and on Stack
  Exchange alike.
- **Stack Exchange.** The search endpoint's tokenisation of `_` isn't documented. Only half the
  functions were sampled, because of the daily quota.
- **H3a.** Private and unindexed projects are invisible, and the GitHub dependency graph lists
  only 65 of 105 dependents. The count is a lower bound on exposure. It's also an upper bound
  on breakage among visible projects, since none of them breaks.
- **H8.** The best-match order favours files where the term is prominent. One-argument calls on
  array-typed columns or variables outside assignments are counted as aggregates. ST_Union over
  rasters is counted with geometries.

## Reproduction

```sh
cd experiments/e6-usage
# H15 counts (resumable; about 6.5 s per request; SQL+PLpgSQL take about 65 min)
uv run --with requests python fetch_h15.py SQL PLpgSQL
uv run --with requests python fetch_h15.py Python          # secondary, optional
uv run --with requests python fetch_se.py                  # 153 Stack Exchange requests
uv run --with requests --with scipy python fetch_deep.py   # deep-page precision check
uv run --with requests --with scipy python analyze_h15.py  # writes usage.csv, prints tables
# H3a
./dependents.sh
uv run --with requests python fetch_h3a.py
# then shallow-clone the repositories listed in this document and grep (see H3a method)
# H8
uv run --with requests --with scipy python h8_aggregate_vs_scalar.py
```

Every HTTP response is cached under `target/experiments/e6/`, keyed by request, so rerunning
the analysis scripts makes no network calls. The GitHub token comes from `gh auth token`.
