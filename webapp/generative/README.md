# Generative UI mode

`webapp/` に統合された Streamdown の Generative UI モードです。

通常の Viewer は `../`、Generative UI はこのディレクトリから開きます。`../build.sh` は既存 renderer WASM と core parser WASM の両方を生成し、Generative UI が使う `streamdown.wasm` / `streamdown.js` をここへ配置します。

対応する `:::llm ui` component:

- `layout`, `tabs`, `form`
- `metric`, `chart`, `graph`, `canvas`, `progress`
- `slider`, `input`, `select`, `button`
- `derive`, `state`, `patch`, `when=` による reactive state / component updates

任意 JavaScript や HTML は実行しません。button/form action は `set`, `increment`, `decrement`, 明示操作型 `llm:` の allowlist、式は専用の安全な式評価器、canvas は固定描画DSLのみです。

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

For stateless proxies, the interaction prompt also contains a bounded semantic inventory of the current generated UI (up to 64 components). Only whitelisted descriptor fields such as `type`, `id`, `label`, `title`, `state`, `tab`, `when`, and `unit` are included. Component bodies, `action` strings, arbitrary attributes, and sensitive-looking state names are not included. This gives the model enough structure to extend the existing screen without retransmitting raw DOM or executable content.

The browser smoke covers the complete interaction path:

```text
explicit generated button click
  -> bounded local state snapshot
  -> Chat-compatible POST proxy
  -> SSE deltas
  -> Streamdown WASM append-after-finish
  -> new semantic UI appended to the existing application
```

## Declarative state patches (`type=state`)

LLM continuationは、新しいカードを追加するだけでなく既存UIの共有stateを安全に更新できます。

```md
:::llm ui type=state
temperature=58
mode=exact
alerts=true
:::
```

`state` block自体は描画されません。fenceが完全に閉じた時だけpatch全体を原子的に適用するため、token途中の `temperature=5` を最終値と誤認して副作用を起こしません。同じblock/signatureは再適用されないので、その後のslider操作を古いpatchが巻き戻すこともありません。

1 blockあたり最大32項目で、keyは限定された識別子形式のみです。空値と `password` / `secret` / `token` / `api-key` / `credential` / `auth` 系keyを拒否し、値は最大512文字のstring、有限number、boolean、`null` のprimitiveに正規化します。適用後は同じ共有state経路を通るため、slider、progress、`{{state}}` binding、`derive`、`when=`、selectが同時に更新されます。

## Safe component overlays (`type=patch`)

LLM continuationは、既存componentをID指定で安全にoverlayできます。DOM selectorやJavaScriptは受け付けません。

```md
:::llm ui type=patch target=throughput
label=Model-updated throughput
value=3.1M
trend=patched safely
:::
```

patchはfenceが完全に閉じた時だけ反映され、複数patchは文書順にmergeされます。後続patchは同じ属性だけを上書きします。truncateでpatch blockが消えた場合はoverlayも消え、元のcomponent記述へ戻ります。

変更可能なのは `label`, `title`, `value`, `unit`, `trend`, `min`, `max`, `step`, `options`, `values`, `placeholder`, `when`, `height`, `width` のbounded文字列だけです。`action`, `state`, `type`, `id`, `target`, `tab`, `span`, endpoint, HTTP header, credentialは変更できません。したがってmodel responseは既存UIの見た目や安全な入力候補を更新できますが、実行権限や接続先を昇格できません。
