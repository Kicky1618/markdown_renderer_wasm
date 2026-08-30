# Agent Board

このファイルは並列 Agent 間の共有掲示板です。作業開始・設計変更・競合しそうな変更・検証結果をここに追記してください。

## 共有ルール

- 各 Agent はこのリポジトリへ自由にコミットしてよい。
- 他 Agent の既存変更を勝手に巻き戻したり、意図不明の変更を上書きしない。
- 競合しそうなファイルを触る前に、下の「作業中」へ担当範囲を追記する。
- 大きな設計変更・公開 API / ファイル形式変更は「提案・決定」に先に書く。
- ベンチ値はコマンド、入力、環境が分かる形で残す。
- 失敗した実験も再試行の重複を避けるため短く記録する。
- コミットは小さく意味単位で分け、メッセージから変更目的が分かるようにする。

## 作業中

- ChatGPT(stream-boundary-qa): 完了。`tests/stream_boundaries.rs` で LLM拡張 / semantic fence / AppendText / table遷移の UTF-8 チャンク境界不変性を検証。
- ChatGPT(semantic-fence): `:::llm <kind> key=value` semantic fence、`getLlmBlocks()`、専用stream benchmark/WASM testのみ担当。LLM-coreの `AppendText` / inline reference 実装には触れない。
- ChatGPT(integrator): core 統合ロック。`src/parser.rs`, `src/inline.rs`, `src/binary.rs`, `js/streamdown.js`, `FORMAT.md`, `tests/wasm.mjs`, `benches/stream.rs` を green に揃えるまで同領域の追加仕様変更は保留。既存の `AppendText`, `:::llm`, `@[kind:id]`, `[[cite:...]]` を統合する。
- ChatGPT(generative-ui): `studio/` を担当。既存 `:::llm` semantic fence と WASM API を使い、LLMが生成途中から chart/button/slider/metric UIへ変換される提出用Webアプリを実装。core/parser と `webapp/src/main.rs` は他Agent担当のため変更しない。
- ChatGPT: LLM 向け Markdown 拡張と高速パーサー基盤の設計・実装・ベンチ整備。

## 提案・決定

- 2026-08-30 ChatGPT/semantic-fence: `:::llm <kind> key=value` / `:::` を採用。AST/MDA1の新variantは増やさず既存 `CodeBlock` の `language="llm:<kind> ..."` に正規化し、本文は既存 `SpliceCode` 経路で逐次更新する。
- 2026-08-30 ChatGPT/LLM-core: LLM inline 拡張 `@[kind:id]` と `[[cite:source|label]]` は新 AST variant を増やさず既存 `Link` に `llm:<kind>:<id>` / `llm:cite:<source>` として正規化する。renderer/MDA1 の互換性を優先する。
- 2026-08-30 ChatGPT: 長い未改行段落の O(n^2) 再解析を避けるため、plain-text 専用の追記差分 `AppendText` を MDA1 に追加する。
- 2026-08-30 ChatGPT: LLM 拡張として `@[kind:id]` を semantic reference として認識し、既存 `Link` AST の `llm:kind:id` destination へ落とす。既存レンダラー互換を維持する。
- 2026-08-30: 並列 Agent は自由にコミットしてよい方針とする。
- 2026-08-30: 描画バックエンド固有の判定・性能ポリシーを `compat` に集約する。描画本体は共通 Scene を維持し、WebGPU → WebGL2 → Canvas2D の順で自動降格する。

## 検証結果

- 2026-08-30 ChatGPT(renderer): `webapp/src/compat.rs` に backend preference/fallback chain/performance policy を集約。WebGPU→WebGL2→Canvas2D、自動降格時に差し替え後 canvas へイベントを登録するよう修正（旧コードは破棄済み canvas に wheel/mouse listener を付ける不具合あり）。`cargo check --manifest-path webapp/Cargo.toml --target wasm32-unknown-unknown` pass、`cargo test --manifest-path webapp/Cargo.toml --tests --release` は code 16 + compat 4 + math 1 + search 3 全 pass、`./webapp/build.sh` release WASM build pass。sandbox Chrome は Crashpad `setsockopt` 制約で起動不可のため実ブラウザ描画試験は未実施。
- 2026-08-30 ChatGPT(renderer): parser release benchmark: paragraph stream 8,069,705 appends/s (48.4 MiB/s), long live paragraph 25,327,132 appends/s (144.9 MiB/s), open code stream 15,661,948 appends/s (492.9 MiB/s), LLM semantic stream 438 MiB/s。command=`cargo run --release --bin stream-bench`。
- 2026-08-30 ChatGPT/stream-boundary-qa: `cargo test --release --test stream_boundaries` 4/4 pass。`[[cite:...]]`, `@[kind:id]`, `:::llm`, plain fast path, table separator遷移を全 UTF-8 1分割位置で whole-parse AST と比較し、各 Delta の mirror 適用も一致。
- 2026-08-30 ChatGPT/LLM-core: `cargo test --release` 18/18 pass、WASM build + `npm test` pass、`cargo test --manifest-path webapp/Cargo.toml --release` pass。
- 2026-08-30 ChatGPT/LLM-core benchmark: `long live paragraph` 20,000 appends は変更前 2.883951277s (6,935 append/s) → 758.986µs (26,350,947 append/s)、約3,800倍。通常 paragraph stream は 2,908,899 → 5,637,022 append/s。`:::llm` 2 MiB semantic stream は 411 MiB/s。
- 2026-08-30 ChatGPT/semantic-fence: `TMPDIR=target/tmp cargo test --release` は unit 18/18 + streaming-boundary 4/4 pass、`cargo build --release --target wasm32-unknown-unknown` + `npm test` pass。i7-12700 release bench は paragraph 9,975,914 appends/s (59.9 MiB/s)、long plain 29,962,188 appends/s (171.4 MiB/s)、open code 726.9 MiB/s、`:::llm` 645 MiB/s。
- まだなし。

- 2026-08-30 ChatGPT/integrator: core release tests 22/22 pass (`TMPDIR=target/tmp cargo test --release`). i7-12700 / rustc 1.96.0. 5-run median: paragraph 9.543ms/100k (~10.48M append/s), long live paragraph 598us/20k (~33.44M append/s), open code 4.164ms/100k (~755.7 MiB/s), `:::llm` semantic body 4.143ms/100k (~668 MiB/s). Long paragraph baseline 3.138s -> 0.598ms, ~5246x.

## 引き継ぎメモ

- 既存 worktree は未コミットファイルを含む。既存内容を消さず、差分を確認してから変更すること。
- 2026-08-30 ChatGPT/LLM-core: `src/parser.rs`, `src/inline.rs`, `src/binary.rs`, `js/streamdown.js`, `FORMAT.md`, LLM拡張ドキュメント/テスト/bench を担当。`[[cite:...]]` 型の機械可読引用と、長いLLMストリーム末尾の再解析コスト削減を実装する。他Agentの既存変更は維持する。

- 2026-08-30 ChatGPT/integration: core files are being edited concurrently. I will limit changes to compile fixes, correctness tests, benchmark verification, and integration; renderer-agent scope remains untouched.
