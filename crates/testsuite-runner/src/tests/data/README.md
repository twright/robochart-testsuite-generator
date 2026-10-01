# Test input rows

The `pipeline_properties` tests generate projects from these files. Each row
is copied unchanged, with its header, from the ExpressionTests input
(`data/` of the RoboChart Python testsuite,
[robochart-python-testsuite](https://github.com/twright/robochart-python-testsuite)):

| File | Rows | Used by |
| --- | --- | --- |
| `cases.csv` | `lambda_False/000` | `lambda_graph_context_distinguishes_wrong_values` |
| | `add/000` | `integer_bounds_include_intermediate_negation` |
| | `tuple/000`, `vector/000`, `matrix/000`, `comprehension_False_False/000`, `comprehension_False_True/002`, `comprehension_True_True/004`, `range_False_True_False/003`, `divide/000` | `structured_values_and_finite_domains_match_recorded_observations` |
| | `empty_sequence/000`, `sequence/000`, `concat/005`, `caret_concat/005` | `core_sequences_work_in_expression_and_statement_contexts` |
| `statement_cases.csv` | `sequence_size/pass`, `sequence_concat/pass`, `sequence_index/pass`, `sequence/pass`, `sequence/mismatch` | `core_sequences_work_in_expression_and_statement_contexts` |
| `supplementary_expression_cases.csv` | all three rows | `supplementary_expression_csv_is_an_independent_corpus` |

The tests check properties of the generator and the runner; they do not
check the testsuite these rows come from. Keep each row identical to its
source so that a test's expected values remain recorded observations.
