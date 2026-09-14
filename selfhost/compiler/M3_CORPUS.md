# M3 parser corpus

The M3 smoke corpus reuses the 70 lexer/parser fixtures under
`crates/kolom-cli/tests/golden`. Do not copy these sources into the self-host
directory: the Rust fixtures remain the language oracle.

## Coverage map

| Parser area | Fixtures |
|---|---|
| Basic declarations and expressions | `01_hello`, `02_variables`, `03_functions`, `05_strings`, `07_arrays`, `08_const_float`, `38_mixed_arithmetic` |
| Control flow and statement context | `04_control_flow`, `19_ok_for_each`, `20_ok_joth`, `36_continue`, `48_compound_assign` |
| Types, structs, maps, and modules | `28_user_module`, `29_map_type`, `30_struct_type`, `32_nested_struct`, `33_struct_fn`, `34_module_struct`, `39_struct_containers`, `53_generics`, `59_enum_in_struct`, `62_generic_payload_field` |
| Try/catch and extern | `31_try_catch`, `35_try_nested`, `54_extern_ffi` |
| Enums, match, and functions as values | `51_enum_match`, `52_first_class_fn`, `56_unreachable_match_arm`, `61_indirect_call`, `64_err_bare_constructor` |
| Contextual widgets and names | `27_stdlib_graphics`, `65_contextual_names` |
| Valid parser stress cases | `37_empty_array_and_const`, `40_print_containers`, `50_json_dom`, `55_module_stress`, `58_array_push`, `67_char_code` |
| Invalid syntax and recovery | `09_err_lex`, `60_err_enum_struct_cycle`, `63_err_not_callable`, `66_err_struct_enum_same_name`, `68_err_mixed_digit_literal`, `69_bracket_mismatch`, `70_block_comment` |

## M3 acceptance

For every fixture, run the self-hosted parser driver against `main.ক`'s input
file and record:

- no hang or crash;
- lexer diagnostics separately from parser diagnostics;
- zero diagnostics for fixtures whose name begins with `01` through `08`,
  `13`, `19` through `59`, `61`, `62`, `65`, `67`, and `70` unless the fixture
  intentionally tests a lexer error;
- at least one diagnostic for each `err_` fixture;
- parser output remains deterministic across two runs.

The driver is `selfhost/compiler/main_পার্স.ক`. Execution is intentionally not
part of the Rust test suite because it requires the reference Kolom runtime.