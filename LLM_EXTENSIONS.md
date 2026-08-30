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

## Semantic dependency graph

semantic fence に `id=` と `depends=kind:id,...` を付けると、JavaScript tools 層で依存DAGとして解釈できる。AST / MDA1 形式は変えない。

```md
:::llm tool id=search
{"query":"streaming markdown"}
:::

:::llm artifact id=summary depends=tool:search
{"title":"Streamdown"}
:::

:::llm ui id=result depends=artifact:summary
{"type":"metric"}
:::
```

`tools/semantic-graph.mjs` は未解決依存、重複ID、cycle を検出し、dependency-first `executionOrder` を返す。`SemanticRuntime` / `SemanticScheduler` は Markdown 受信を止めずに、依存先runnerの完了後だけ下流nodeを実行する。

## Streaming state and merge patches

`state` と `patch` は生成UIやtool workflowの共有JSON状態をストリーム内で更新するためのsemantic kindとして利用できる。

```md
:::llm state id=session
{"count":0,"status":"warming"}
:::

:::llm patch id=ready target=state:session depends=state:session if_revision=1
{"count":1,"status":"ready"}
:::
```

`tools/semantic-state.mjs` の既定patchは RFC 7396 JSON Merge Patch。objectは再帰merge、`null`はキー削除、array/scalarは置換となる。`format=replace` ではstate全体を置換する。

各stateは revision 1 から単調増加する。`if_revision=N` を指定したpatchは、適用直前のrevisionがNでなければ `SemanticRevisionConflictError` で失敗するため、並列分岐やstale updateをsilentに上書きしない。同じstateへ複数patchを当てる場合は `depends=patch:<previous>` で直列化するのが基本となる。

JSON payload 内の `__proto__` / `prototype` / `constructor` は拒否し、runner結果とstate snapshotはcloneしてcanonical stateへの外部aliasを作らない。`streamdown-inspect.mjs --validate` はtarget、format、dependency path、revision指定、同一stateへの非直列patchを事前診断する。

## Performance model

LLM 向け拡張は、専用の巨大 AST や JSON 中間表現を増やさず、既存の差分経路を再利用する。

- 素の未改行テキスト: `AppendText`
- Markdownを含む未確定段落 + plain token: `AppendInlineText`（表候補など曖昧な末尾は再解析）
- `:::llm`（3個以上の可変長コロン）/ fenced code body: `SpliceCode`
- 通常 Markdown: 不安定な末尾 suffix のみ `Truncate + Push`
- WASM 境界: JSON ではなく MDA1 バイナリ

2026-08-30、Intel Core i7-12700、release build で `cargo run --release --bin stream-bench` を実行した参考値:

| workload | result |
|---|---:|
| paragraph stream | 10.46M appends/s, 62.8 MiB/s |
| long live plain paragraph | 32.78M appends/s, 187.6 MiB/s |
| formatted live paragraph after `**bold**` | 約 36.88M appends/s, 211 MiB/s |
| multiline formatted paragraph | 約 33.03M appends/s, 189.7 MiB/s |
| open fenced code | 23.58M appends/s, 742.0 MiB/s |
| `:::llm` semantic fence | 682 MiB/s |

値はこの環境でのローカル測定であり、他環境の性能を保証するものではない。
