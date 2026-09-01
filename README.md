# Streamdown

[![Benchmarks](https://github.com/Kicky1618/markdown_renderer_wasm/actions/workflows/benchmarks.yml/badge.svg)](https://github.com/Kicky1618/markdown_renderer_wasm/actions/workflows/benchmarks.yml)
[Bencher Cloud performance history](https://bencher.dev/perf/markdown-renderer-wasm)

LLMの逐次出力向けに、末尾追記だけを処理する依存ゼロのRust Markdown
パーサーです。HTMLではなく独自ASTの差分を返します。

通常の入力では未確定な末尾ブロックだけを `Truncate + Push` で置換します。
LLMが長い通常文を改行なしで生成するときは `AppendText` で末尾テキストだけを
追記し、段落全体の再解析・再コピーを避けます。フェンスコードでは `SpliceCode` を
使い、数MBのコードブロックでも新しいチャンクと現在行だけを差分に含めます。
JSONシリアライズは行わず、WASM境界は小さな `MDA1` バイナリ形式です。

コンポーネント構成、逐次解析の状態管理、WASM境界、Canvasデモの描画系は
[DESIGN.md](DESIGN.md) にまとめています。

## Build and test

```sh
cargo test --release
rustup target add wasm32-unknown-unknown
./scripts/build_wasm.sh
```

生成物は `target/wasm32-unknown-unknown/release/streamdown.wasm` です。

```js
import { Streamdown } from "./js/streamdown.js";

const parser = await Streamdown.load(fetch("./streamdown.wasm"));
const changes = parser.append("# Hello\n\n生成中の **Markdown");
// parser.document は適用済みAST、changes は今回分だけ
parser.append("** です。\n");
parser.dispose();
```

AIチャットでは、`Response`、`ReadableStream`、Async Iterableをそのまま
`consume`へ渡せます。UTF-8文字がチャンク境界をまたいでも安全に復元されます。

```js
const controller = new AbortController();
await parser.consume(fetchResponse, {
  signal: controller.signal,
  onDelta(changes, document) {
    // changesだけをチャットUIのレンダラーへ反映
  },
});
```

主なメソッドとプロパティは次の通りです。

- `append(chunk)` / `appendMany(chunks)`: 1個または複数の文字列チャンクを追記し、Deltaを返す
- `appendInPlace(chunk)`: hot-path Delta objectを生成せず `document` を直接更新する高速経路
- `consume(source, options)`: Response、ストリーム、Iterableを最後まで取り込み。`onDelta` 未指定時は `appendInPlace` を自動使用
- `finish()`: 最終チャンクを確定（末尾改行のないコードフェンスにも対応）
- `reset()`: 次のアシスタントメッセージ用に状態を初期化
- `setContent(markdown)`: 再生成・編集後の応答で現在内容を置換
- `snapshot()`: 状態管理へ保存できる独立したASTコピーを取得
- `toPlainText()`: コピー、読み上げ、検索用のプレーンテキストを取得
- `getCodeBlocks(filter)` / `getLinks()`: コードやリンクを抽出
- `getLlmBlocks(filter)` / `getCitations()`: LLM semantic fence・機械可読引用を抽出
- `blockCount` / `isEmpty` / `isDisposed`: 現在状態を取得

ネイティブRustでは `Parser::append(&str) -> Delta`、`Parser::blocks()`、
`Parser::reset()`、`Parser::replace(&str)`、`Parser::is_empty()`を直接利用できます。
バイナリ仕様は [FORMAT.md](FORMAT.md) にあります。

ベンチマークは次で実行します。

```sh
cargo run --release --bin stream-bench
cargo run --release --manifest-path webapp/Cargo.toml --example syntax_highlight_bench
```

`main` への push と pull request では GitHub Actions が自動で7回測定し、
各項目の中央値を Bencher Cloud に記録します。GitHub-hosted runner のCPU差を
性能退行と誤認しないよう、CPU型番・vCPU数・architectureごとに Bencher testbed を
自動分離します。`main` では同一testbedの履歴が3点以上たまると、直近8点に対して
10%以上低下した項目を alert にします。生ログ、BMF JSON、集計表は Actions artifact に
14日間保存します。

## Supported syntax

ATX見出し、段落、強調、太字、インラインコード、リンク、改行、引用、順序・
非順序リスト、水平線、言語付きフェンスコード、パイプテーブルを扱います。HTML、生HTML、
ネストしたリスト、脚注は意図的に含めていません。CommonMark完全互換よりも、
LLMが通常生成する構文で、ブロック単位の再解析と小さい差分を優先しています。

### LLM extensions

通常のMarkdownに加えて、逐次生成とRAG/Tool利用を想定した軽量な拡張を扱います。

```md
回答本文 [[cite:spec-42|仕様書]] と @[source:turn7search2]

:::llm tool name="web search" id=q1
{"query":"rust wasm"}
:::
```

- `:::llm <kind> [key=value ...]` ... `:::`: tool / artifact / metric などの raw payload をストリームする semantic fence。既存 `CodeBlock` AST に `language="llm:..."` として落とすため互換性を維持します。
- `@[kind:id]`: 機械可読参照。既存 `Link` AST の `llm:<kind>:<id>` destination に正規化します。
- `[[cite:source]]` / `[[cite:source|label]]`: RAG引用。`llm:cite:<source>` への `Link` として保持します。
- 長いプレーン段落は `AppendText` 差分を使い、既出prefixを毎チャンク再解析・再転送しません。

JavaScript側では `getLlmBlocks()` と `getCitations()` で構造化情報を直接取得できます。

詳細な構文・正規化規則・性能モデルは [LLM_EXTENSIONS.md](LLM_EXTENSIONS.md) を参照してください。

## Canvas demo

`webapp/` に、36 TPSのLLMモックをこのパーサーへ流し、ASTを `wgpu` で
単一Canvasへ描画するデモがあります。Markdown本文用のDOMノードは生成しません。

```sh
./webapp/build.sh
python3 -m http.server 8080 --directory webapp
```

WebGPU対応ブラウザで `http://localhost:8080` を開き、ホイールまたは
トラックパッドでスクロールできます。

描画バックエンドはWebGPU、WebGL2、Canvas2Dの順に自動フォールバックします。
`?renderer=webgpu`、`?renderer=webgl`、`?renderer=canvas2d`で明示的に開始バックエンドを
選択できます。WebGL2ではWebGPU版と同じAST・レイアウト・ハイライトを使用し、
Canvas2Dでも共通ParserのAST、数式、コードハイライト、リスト、表を描画します。
Canvas2Dでも本文余白からのテキスト選択、コピー、スクロールバー操作が可能です。

`http://localhost:8080/?tps=1000000&repeat=10000&autoscroll=1` のように、
TPS、モック文書の繰り返し回数、自動スクロールを指定できます。上方向へ
スクロールすると自動追従を停止し、最下部へ戻ると再開します。
右端のスクロールバーもCanvasへ直接描画され、クリック・ドラッグできます。
URLでは`fontsize=16&fade=180`で表示を調整でき、`doc=easy`で`easy_test.md`、
`doc=stress`で`math_stress_test.md`を表示します。
`doc=code`ではコードブロックのハイライト試験文書を表示します。
コードフェンスの生成中は割り当てなしの1パス字句解析で軽量に色分けし、閉じた後は
複数行コメント、関数、型、マクロ、演算子まで含めて再ハイライトします。
巨大なコードブロックも画面周辺の行だけをGPUへ送るため、行数に比例して描画量が増えません。
`$E=mc^2$`、`$$\frac{a}{b}$$`、`\sqrt{x}`などの数式はRaTeXでパース・組版し、
埋め込みKaTeXフォントを使った透明PNGからRGBAへ展開して表示します。表示数式は
Shift+ホイールで横スクロールできます。

本文はNoto Sans CJK JP、コードブロックはNoto Sans Monoを使い、アウトラインから
ラスタライズしたアンチエイリアスcoverage付きグリフをキャッシュします。
Canvas2Dでも同じWebフォントを使用します。
JIS X 0208の日本語、Latin、Greek、Cyrillic、主要記号を収録しています。
描画位置は整数ピクセルへスナップするため、
スクロール中も文字がぶれません。
