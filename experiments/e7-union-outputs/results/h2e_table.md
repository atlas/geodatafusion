| kind | source | construct | DataFusion 54 | DataFusion 55 |
|---|---|---|---|---|
| union | column | control: projection | yes | yes |
| union | column | CASE (two branches) | NO | NO |
| union | column | CASE (ELSE NULL) | NO | NO |
| union | column | COALESCE(x, x) | NO | NO |
| union | column | COALESCE(x, NULL) | NO | NO |
| union | column | make_array + unnest | NO | NO |
| union | column | make_array (element field) | NO | NO |
| union | column | array_agg + unnest | NO | NO |
| union | column | array_agg (element field) | NO | NO |
| union | column | UNION ALL | yes | yes |
| union | column | CAST to own storage type | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 29") | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 29") |
| union | column | arrow_cast to own storage type | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 198") | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 198") |
| union | column | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| union | udf | control: projection | yes | yes |
| union | udf | CASE (two branches) | NO | NO |
| union | udf | CASE (ELSE NULL) | NO | NO |
| union | udf | COALESCE(x, x) | NO | NO |
| union | udf | COALESCE(x, NULL) | NO | NO |
| union | udf | make_array + unnest | NO | NO |
| union | udf | make_array (element field) | NO | NO |
| union | udf | array_agg + unnest | NO | NO |
| union | udf | array_agg (element field) | NO | NO |
| union | udf | UNION ALL | yes | yes |
| union | udf | VALUES (constants) | yes | yes |
| union | udf | CAST to own storage type | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 40") | error: SQL error: ParserError("Expected: a data type name, found: , at Line: 1, Column: 40") |
| union | udf | arrow_cast to own storage type | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 209") | error: SQL error: ParserError("Expected: ), found: vertices at Line: 1, Column: 209") |
| union | udf | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| wkb | column | control: projection | yes | yes |
| wkb | column | CASE (two branches) | NO | NO |
| wkb | column | CASE (ELSE NULL) | NO | NO |
| wkb | column | COALESCE(x, x) | NO | NO |
| wkb | column | COALESCE(x, NULL) | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w |
| wkb | column | make_array + unnest | NO | NO |
| wkb | column | make_array (element field) | NO | NO |
| wkb | column | array_agg + unnest | NO | NO |
| wkb | column | array_agg (element field) | NO | NO |
| wkb | column | UNION ALL | yes | yes |
| wkb | column | CAST to own storage type | yes | NO |
| wkb | column | arrow_cast to own storage type | udf only | NO |
| wkb | column | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| wkb | udf | control: projection | yes | yes |
| wkb | udf | CASE (two branches) | NO | NO |
| wkb | udf | CASE (ELSE NULL) | NO | NO |
| wkb | udf | COALESCE(x, x) | NO | NO |
| wkb | udf | COALESCE(x, NULL) | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w |
| wkb | udf | make_array + unnest | NO | NO |
| wkb | udf | make_array (element field) | NO | NO |
| wkb | udf | array_agg + unnest | NO | NO |
| wkb | udf | array_agg (element field) | NO | NO |
| wkb | udf | UNION ALL | yes | yes |
| wkb | udf | VALUES (constants) | yes | yes |
| wkb | udf | CAST to own storage type | yes | NO |
| wkb | udf | arrow_cast to own storage type | udf only | NO |
| wkb | udf | CAST to VARCHAR (should drop) | KEPT (wrong); tag kept in plan; exec error | dropped (correct); no tag in plan; exec error |
| plain | column | control: projection | n/a (control, works) | n/a (control, works) |
| plain | column | CASE (two branches) | n/a (control, works) | n/a (control, works) |
| plain | column | CASE (ELSE NULL) | n/a (control, works) | n/a (control, works) |
| plain | column | COALESCE(x, x) | n/a (control, works) | n/a (control, works) |
| plain | column | COALESCE(x, NULL) | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w | error: Error during planning: Execution error: Function 'coalesce' user-defined coercion failed w |
| plain | column | make_array + unnest | n/a (control, works) | n/a (control, works) |
| plain | column | make_array (element field) | n/a (control, works) | n/a (control, works) |
| plain | column | array_agg + unnest | n/a (control, works) | n/a (control, works) |
| plain | column | array_agg (element field) | n/a (control, works) | n/a (control, works) |
| plain | column | UNION ALL | n/a (control, works) | n/a (control, works) |
| plain | column | CAST to own storage type | n/a (control, works) | n/a (control, works) |
| plain | column | arrow_cast to own storage type | n/a (control, works) | n/a (control, works) |
| plain | column | CAST to VARCHAR (should drop) | n/a (control, exec error); no tag in plan; exec error | n/a (control, exec error); no tag in plan; exec error |
