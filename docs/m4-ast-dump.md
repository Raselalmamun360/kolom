# M4 stable AST dump

M4 compares the Rust parser with the self-hosted parser without relying on
Rust's `Debug` formatting. Both sides emit one tab-separated record per AST
node, in source order.

## Record format

```text
NODE<TAB>kind<TAB>line<TAB>column<TAB>payload
```

`kind` and `payload` use ASCII names. Text values are escaped with the same
rules as `lex --stable`: backslash, tab, newline, carriage return, C0, and
DEL are escaped; all other Unicode text is emitted as UTF-8. Empty payloads
still end with the final tab.

Containers are delimited by explicit records:

```text
NODE	begin_program	0	0	
NODE	ident	1	1	ইম্পোর্ট_নাম
...
NODE	end_program	0	0	
```

The complete node vocabulary is the AST vocabulary, not an implementation
detail. Container records use `begin_<kind>` and `end_<kind>`. Every record includes source position when its node has one. Lists
preserve source order. Enum tags and operator names use these stable names:
`lit`, `ident`, `qualified`, `unary`, `postfix`, `binary`, `assign`,
`field_assign`, `match`, `var`, `const`, `if`, `loop`, `while`, `foreach`,
`return`, `break`, `continue`, `expr`, `nested`, `try`, `widget`, `display`,
`type_named`, `type_array`, `type_shared`, `type_map`, `type_func`, and
`type_generic`.

The Rust CLI command is `kolom ast --stable <file>`. The Rust dumper is in
`crates/kolom-cli/src/stable_ast.rs`. A parse or lex error is
reported as `ERR<TAB>phase<TAB>line<TAB>column<TAB>message` and the command
returns failure. A successful dump must contain no `Debug` formatting.

The self-hosted dumper must emit the same records from the structures in
`selfhost/compiler/এস্ট.ক`. M4 is complete only when all 70 golden fixtures
match at the lexer, AST, and diagnostic levels.