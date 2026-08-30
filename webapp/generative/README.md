# Generative UI mode

`webapp/` に統合された Streamdown の Generative UI モードです。

通常の Viewer は `../`、Generative UI はこのディレクトリから開きます。`../build.sh` は既存 renderer WASM と core parser WASM の両方を生成し、Generative UI が使う `streamdown.wasm` / `streamdown.js` をここへ配置します。

対応する `:::llm ui` component:

- `layout`, `tabs`, `form`
- `metric`, `chart`, `graph`, `canvas`, `progress`
- `slider`, `input`, `select`, `button`
- `derive` と `when=` による reactive state

任意 JavaScript や HTML は実行しません。button/form action は `set`, `increment`, `decrement` の allowlist、式は専用の安全な式評価器、canvas は固定描画DSLのみです。

```sh
./webapp/build.sh
python3 -m http.server 8080 --directory webapp
```

- Viewer: `http://localhost:8080/`
- Generative UI: `http://localhost:8080/generative/`

Generative UI のテストは build 後に次で実行できます。

```sh
node webapp/generative/tests.mjs
```

## HTTP / LLM streaming input

左ペインの `HTTP stream` から、CORS許可されたHTTP(S) endpointまたは同一origin proxyへ接続できます。ブラウザ内にAPIキーを保存・注入する機能は持ちません。

- `Auto`: `Content-Type` から判定
- `SSE`: `text/event-stream` の `data:` envelopeを除去
- `NDJSON`: 1行1JSONを逐次処理
- `Plain text`: body chunkをそのままMarkdownへ追加

SSE/NDJSONでは `choices[0].delta.content`、Responses API系の `delta`、`delta.text`、`output_text` など代表的なLLM delta envelopeから文字列だけを抽出し、既存の `Parser::append` に渡します。`[DONE]` も認識します。

接続方式は `GET stream` に加えて `Chat-completions proxy` / `Responses proxy` を選べます。POSTモードではpromptとmodelを入力し、runtimeがGenerative UIの安全な構文説明をsystem promptとして自動付与します。Chat互換proxyには `messages`、Responses互換proxyには `instructions` / `input` を送ります。

実運用では、認証情報を持つサーバ側proxyがLLM providerへ接続し、ブラウザにはMarkdown/SSEだけを返す構成を推奨します。ブラウザ側のfetchは `credentials: omit` で、API key入力欄もありません。

POST→SSE→WASM parser→UI生成まで含む実ブラウザsmokeは、build後に次で実行できます。

```sh
sh webapp/generative/browser_smoke.sh
```

fixtureはsystem promptにStreamdownの構文・JavaScript禁止制約が含まれること、user promptとmodelが正しいことまで検証してからSSEを返します。

## Streaming graph

`type=graph` は依存DAGや処理パイプラインを逐次可視化します。本文は固定DSLで、最大128 node / 256 edgeです。DOM/SVGはruntime側が生成し、LLM出力をHTMLとして解釈しません。

```md
:::llm ui type=graph id=pipeline title="Runtime pipeline" span=2
node llm LLM tokens
node parser Incremental parser
node delta Delta AST
node ui Generative UI
edge llm parser stream
edge parser delta diff
edge delta ui apply
:::
```

`node <id> <label...>` と `edge <from> <to> <label...>` のみを認識し、DAGは決定的なlayered layout、cycleは末尾layerへ安全に配置します。

## Visible latency metrics

ヘッダには各sessionの `chars/s` と `first UI` を表示します。`first UI` はreset/接続開始から最初の生成component (`ui-card` / layout / tabs / form) がDOMへ現れるまでの時間です。HTTP/SSE入力でも同じ計測経路を使うため、パーサーの逐次性をデモ画面上で確認できます。

## Model round trips (`action=llm:`)

Generated buttons/forms can explicitly ask the currently configured POST proxy to continue the application:

```md
:::llm ui type=button id=refine
label=Generate next view
action=llm:Use the current state to append one compact recommendation card
:::
```

This only runs after a user click/submit. The generated Markdown cannot choose the endpoint, model, headers, credentials, or HTTP method; the runtime reuses the connection settings the user configured in the left pane. The continuation is appended through the same `Parser::append` delta path, so existing state/UI remains live while the new response streams in.

Only a bounded state snapshot is sent: at most 64 primitive values (`string`/finite `number`/`boolean`/`null`), strings are capped at 512 characters, and keys resembling passwords, secrets, tokens, API keys, credentials, or auth values are omitted. Objects/arrays are not serialized.

The browser smoke covers the complete interaction path:

```text
explicit generated button click
  -> bounded local state snapshot
  -> Chat-compatible POST proxy
  -> SSE deltas
  -> Streamdown WASM append-after-finish
  -> new semantic UI appended to the existing application
```
