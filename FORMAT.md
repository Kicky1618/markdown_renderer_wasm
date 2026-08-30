# MDA1 AST delta format

All integers are unsigned 32-bit little-endian. Strings are `u32 byte_length`
followed by UTF-8 bytes. A delta begins with ASCII `MDA1`, then an operation
count.

| Op | Byte | Payload |
|---|---:|---|
| `Truncate` | 1 | first deleted block index |
| `Push` | 2 | one encoded block |
| `SpliceCode` | 3 | block index, UTF-8 tail byte count, appended string |
| `SealCode` | 4 | block index |
| `AppendText` | 5 | paragraph block index, appended UTF-8 string |
| `AppendInlineText` | 6 | paragraph block index, appended UTF-8 string |
| `SpliceInlineTail` | 7 | paragraph block index, UTF-8 bytes removed from final Text, replacement inline vector |

Block tags are paragraph (1), heading (2), fenced code (3), quote (4),
unordered list (5), ordered list (6), thematic break (7), and table (8). Inline tags are
text (1), emphasis (2), strong (3), code (4), link (5), soft break (6), hard
break (7), and math (8). Math stores a one-byte display flag followed by the
unmodified LaTeX source string. The exact field order is specified by
`src/binary.rs`; the reference decoder is `js/streamdown.js`.

Operations are applied in order. `Truncate + Push` replaces the currently
unstable document suffix. `SpliceCode` removes only bytes belonging to the
current unfinished code line and appends its replacement, so delta size stays
proportional to the new chunk rather than total code-block size. `AppendText`
is a fast path for a live paragraph whose AST is exactly one `Text` node; it
avoids reparsing and retransmitting the already-generated paragraph prefix.
`AppendInlineText` extends the same idea to a live paragraph that already contains
formatted inline nodes: it appends to the final `Text` node, or creates one when
the current inline tail is non-text. `SpliceInlineTail` removes a UTF-8 suffix from
the final `Text` inline (dropping that node when it becomes empty) and appends a
small inline vector. This lets a streamed delimiter run become `Code` or `Math`
without retransmitting the whole paragraph. Consumers should coalesce adjacent
`Text` nodes while applying the replacement.

## LLM extensions and wire compatibility

LLM-specific syntax deliberately reuses existing AST nodes, so no new block or
inline tags are required:

- `:::llm <kind> [key=value ...]` ... `:::` is encoded as a fenced code block.
  Its language field is normalized to `llm:<kind> ...` and its body continues to
  use `SpliceCode` / `SealCode` while streaming.
- `@[kind:id]` is encoded as a normal link whose destination is
  `llm:<kind>:<id>`.
- `[[cite:source]]` and `[[cite:source|label]]` are encoded as normal links whose
  destination is `llm:cite:<source>`.

This keeps older MDA1 consumers able to decode the document even when they do
not attach special semantics to the LLM extensions.