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
## Semantic commit barrier

HTTP/LLM responses act as a commit boundary for generated side effects. Ordinary Markdown, metrics, charts, graphs and other newly-created UI can continue to render as tokens arrive, but closed `type=state` and `type=patch` fences are staged while the network response is active. After `Parser::finish()` for that response, all staged state and component overlays are applied in the same JavaScript turn, followed by one reactive-state reconciliation.

This avoids transient screens where, for example, `temperature=58` has already changed a progress bar while a matching component patch later in the same model response has not arrived yet. The document root exposes `data-semantic-commit="staging|committed|clean"` for diagnostics and browser smoke tests. Manual/local source rendering keeps the existing immediate closed-fence behavior.
## Model response Undo / Redo

The preview header keeps a bounded response history for LLM/HTTP transitions. Before a successful remote response mutates the application, the runtime snapshots the current Markdown source plus local primitive state. `Undo model` rebuilds the WASM parser/UI from that previous source and restores the local state snapshot; `Redo` reconstructs the response again. Raw DOM nodes are never serialized into history.

History is intentionally bounded to 8 response snapshots, and snapshots are skipped when the source exceeds 4 MiB. Manual `Render now`, local stream replay, and `Reset demo` establish a new baseline and clear model-response history. The browser interaction smoke verifies `58°C + component patch -> Undo -> 42°C + original component -> Redo -> 58°C + patch` and exposes `data-history-smoke="pass"`.
## Commit Inspector

Each successful model/HTTP transition also produces a bounded semantic commit summary in the preview. The inspector reports source character delta, changed non-sensitive state keys, `type=patch` targets, semantic block count, newly-created UI count, stream chunk count, and measured first-UI latency. It derives this information from the local response/source snapshots; it never serializes DOM nodes or executes model-provided content.

The summary follows Undo/Redo: undo marks the corresponding commit as `undone`, redo returns it to `applied`. Sensitive-looking state names are filtered from the displayed state-key list. The interaction browser smoke currently verifies `1 patch`, `1 new UI`, and `patched: throughput` after the redo path.

## Browser security policy

Generative UI は `index.html` の CSP でも実行面を制限します。`script-src` は同一originと WebAssembly compilation 用の `wasm-unsafe-eval` のみで、JavaScript の `unsafe-eval` は許可しません。`object-src`, `frame-src`, `worker-src`, `media-src`, `base-uri`, `form-action` は無効化し、対応ブラウザでは Trusted Types を script sink に要求します。

Runtime は生成Markdownを `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `document.write` / `new Function` / `eval` へ渡しません。UIはDOM APIとtext nodeから構築し、HTTP接続はユーザーが設定したHTTP(S) endpointだけを使います。

## Runtime policy audit

閉じたsemantic fenceは実行前と同じpolicy関数で監査されます。画面左の `Runtime policy` にはブロック件数と理由が表示されます。未知のaction verb、敏感stateキー、無効stateキー、`type=patch` の権限外フィールドなどは表示上もblockedになり、button/form実行側も同じ `parseSafeAction` 判定を使うため監査と実行が食い違いません。

監査は未完のstreaming fenceには警告を出さず、fenceが閉じた時点で確定します。E2E fixtureでは意図的に `api_token` state更新を混ぜ、Runtime Policyが1件blockしつつ他のstate/patch/Undo/Redoが継続することを実Chromeで検証しています。

## Human review mode

Preview右上の `Review effects` を有効にすると、`action=llm:` の応答は通常Markdown/新規UIをストリーミング表示しつつ、`type=state` と `type=patch` の副作用だけを保留します。応答終了後は `SIDE EFFECTS STAGED` が表示され、`Apply staged effects` で一括commit、`Reject response` で応答前snapshotへ戻せます。

`Review effects` は既定でONです。ただし応答に有効な `type=state` / `type=patch` が無ければReview画面で停止せず、そのまま通常のMarkdown/新規UI responseとして完了します。自動commitが必要な場合だけユーザーが明示的にOFFへ切り替えます。

Review panelはApply前に、機密風state名を除外した `state` 変更キー、`type=patch` のtarget、追加UI数、semantic block数を要約します。 さらに現在値とのsemantic差分も表示し、例として `temperature: 42 → 58` や `throughput.value: 2.4M → 3.1M` をApply前に確認できます。差分は今回のresponse開始block以降だけを対象にし、過去responseのpatchを混ぜません。例: `2 state / 1 patch / 1 new UI · state: temperature, mode · patched: throughput`。要約はモデル応答中のsemantic fenceだけから作り、DOMやcredential値は読みません。

保留中は次のLLM round tripとUndo/Redoをロックします。Applyした応答だけがmodel historyへcommitされ、Rejectはhistoryを増やしません。通信が途中で失敗・中断した場合もsemantic barrierをcommitせず、不完全なモデル応答を応答前snapshotへ戻します。実Chrome smokeでは `42°C / original component` のstaging状態からApplyで `58°C / patched component`、その後もう一度stageしてRejectで `42°C / original component` に戻る一連を検証しています。

## Response budgets

Network/model responses are checked before each decoded text chunk enters the WASM parser or DOM. A single response is capped at 2 MiB of decoded Markdown, 8,192 emitted chunks, and 256 `:::llm ui` blocks. Semantic fence counting is incremental and detects fences split across transport chunks. If a limit is exceeded, the offending chunk is never appended; `action=llm:` and remote replacement flows discard staged semantic side effects and restore the pre-response application snapshot, with the runtime state set to `LIMIT`.


## Local stream replay

`Replay stream` replays the most recently completed decoded model/HTTP stream entirely in memory, through the same `appendChunk()` → WASM incremental AST → Generative UI → Review path. The recorder stores only decoded Markdown chunks and bounded inter-chunk timing; it does not retain endpoint URLs, HTTP headers, credentials, provider envelopes, or API keys. Recordings are capped at 2 MiB / 8,192 chunks and are discarded when truncated.

Replay first reconstructs the original response-start application state, then emits the recorded chunks with accelerated bounded delays. For `action=llm:` responses this reproduces append-after-finish behavior, state patches, component overlays, policy audit and Human Review without another network request. The browser smoke deliberately replaces the configured endpoint with `127.0.0.1:1` before replay and verifies the same `58°C` / patched throughput UI with `data-replay-smoke="pass"`. Replay recordings live only in page memory and are cleared when a new manual baseline is established.

## Stream Timeline

Completed network and replay sessions produce a bounded timeline of semantic milestones without retaining chunk text. The recorder keeps at most 64 change points containing chunk number, cumulative decoded characters, elapsed time, parser block count, newly generated UI count relative to the response-start baseline, and semantic commit state. DOM is rendered only once at session completion, so timeline diagnostics stay off the parser hot path.

The panel reports both the first-UI chunk and cumulative character position (for example `#4 / 294 chars`) in addition to elapsed time. This makes the streaming property visible as both latency and token/output progress. The Chrome replay smoke verifies the timeline through the network-free replay path and exposes `data-timeline-smoke="pass"`.

## Replay determinism check

After a local `Replay stream`, the runtime compares the replay against the original post-stream snapshot across three bounded axes: the exact decoded Markdown source, the response-local semantic block sequence, and non-sensitive primitive local state. Source expectation is reconstructed from the existing decoded-chunk recording rather than storing a second source copy; semantic attributes are normalized before comparison and credential-like state keys are excluded.

The Timeline panel reports `VERIFIED` only when all three axes match and shows `source ✓ · semantic ✓ · state ✓`; otherwise it reports `DIVERGED` with the failing axis. The Chrome replay smoke currently verifies the full network-free replay as `data-determinism-smoke="pass"` and `data-replay-determinism="verified"`.

When replay diverges, the verifier also reports the first source character mismatch, first semantic-block mismatch, or bounded non-sensitive state-key differences. A dedicated Chrome smoke deliberately records with Review OFF and replays the identical decoded stream with Review ON; it verifies `source ✓ · semantic ✓ · state ✗ · state mismatch: fahrenheit, mode, temperature` with `data-determinism-divergence-smoke="pass"`.

The divergence panel expands those failures without dumping the model body: source failures show mismatch position and expected/actual source lengths, semantic failures show bounded `type/id/target/closed` identity plus body lengths, and state failures show non-sensitive primitive `expected → actual` values such as `temperature: 58 → 42`.

For replay sessions paused by Human Review, the initial comparison intentionally reflects unapplied side effects and may show `source ✓ · semantic ✓ · state ✗`. The pending replay keeps its bounded expected snapshot; after `Apply staged effects`, the runtime immediately compares again and upgrades the same panel to `VERIFIED` when state converges. The Chrome divergence smoke verifies this `DIVERGED → Apply → VERIFIED` transition with `data-determinism-reconcile-smoke="pass"`.
