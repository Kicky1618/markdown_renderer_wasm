# Streamdown LLM Extensions

Streamdown は通常の Markdown に加えて、LLM の逐次出力で機械可読な情報を低コストに運ぶための拡張を持つ。

## Semantic fence: `:::llm`

構文:

```md
:::llm <kind> key=value key="value with spaces"
<body>
:::
```

本文に `:::` 単独行が含まれる場合は、コードフェンスと同様に開始側のコロンを増やせる。終了側は開始時と同数以上のコロンを使う。

```md
::::llm artifact mime=text/plain
alpha
:::
omega
::::
```

例:

```md
:::llm tool name=search id=q1
{"query":"rust wasm parser"}
:::

:::llm artifact mime=application/json name="execution plan"
{"steps":["parse","render"]}
:::
```

`kind` は `tool`、`artifact`、`metric`、`ui` など任意の識別子を利用できる。属性は空白区切りの `key=value` で、値は `"..."` または `'...'` で引用できる。

コア AST / MDA1 に専用 variant は追加しない。開始行は内部的に既存 `CodeBlock` へ正規化される。

- `:::llm tool name=search id=q1` → `language = "llm:tool name=search id=q1"`
- 本文 → `CodeBlock.text`
- `:::` → `closed = true`

このため既存レンダラーは未対応でも通常のコードブロックとして扱える。JavaScript 側では `Streamdown#getLlmBlocks()` が `kind`、`attributes`、`value`、`closed` に構造化する。

```js
const blocks = parser.getLlmBlocks({ kind: "tool", closed: true });
for (const block of blocks) {
  console.log(block.attributes.name, block.value);
}
```

本文は通常のコードフェンスと同じ `SpliceCode` 経路を使う。オープン中の巨大な tool result / artifact payload をチャンクごとに全文再解析せず、現在行の末尾だけ更新する。

## Machine-readable citations

RAGや検索結果の根拠は `[[cite:source]]` または `[[cite:source|label]]` で表現できる。

```md
この仕様は [[cite:doc-42|設計書]] に基づく。
```

ASTでは新variantを増やさず通常の `Link` へ正規化する。

- `[[cite:doc-42]]` → label `doc-42`, destination `llm:cite:doc-42`
- `[[cite:doc-42|設計書]]` → label `設計書`, destination `llm:cite:doc-42`

JavaScriptでは `Streamdown#getCitations()` で `block`、`source`、`label` を抽出できる。
未完の `[[cite:` がチャンク境界に現れた場合も、通常のlive suffix再解析へフォールバックするため、チャンク分割でAST結果は変わらない。

## Semantic inline references

インラインの機械可読参照には `@[kind:id]` を利用できる。

```md
この結果は @[source:turn7search2] を参照した。
```

既存レンダラー互換のため、AST では通常の `Link` に正規化し、destination を `llm:<kind>:<id>` とする。

ネイティブ Rust で3種類の拡張をまとめて確認する例は次で実行できる。

```sh
cargo run --release --example llm_stream
```

## Performance model

LLM 向け拡張は、専用の巨大 AST や JSON 中間表現を増やさず、既存の差分経路を再利用する。

- 素の未改行テキスト: `AppendText`
- `:::llm`（3個以上の可変長コロン）/ fenced code body: `SpliceCode`
- 通常 Markdown: 不安定な末尾 suffix のみ `Truncate + Push`
- WASM 境界: JSON ではなく MDA1 バイナリ

2026-08-30、Intel Core i7-12700、release build で `cargo run --release --bin stream-bench` を実行した参考値:

| workload | result |
|---|---:|
| paragraph stream | 10.46M appends/s, 62.8 MiB/s |
| long live plain paragraph | 32.78M appends/s, 187.6 MiB/s |
| open fenced code | 23.58M appends/s, 742.0 MiB/s |
| `:::llm` semantic fence | 682 MiB/s |

値はこの環境でのローカル測定であり、他環境の性能を保証するものではない。
