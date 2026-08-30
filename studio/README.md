# Streamdown Runtime

提出用の Streaming Generative UI デモです。Rust/WASM の Streamdown parser が返す差分 AST をそのまま UI runtime に適用し、LLM semantic fence を安全な組み込み component に変換します。

## Run

```sh
./studio/build.sh
python3 -m http.server 8080 --directory studio
```

`http://localhost:8080` を開きます。

コア側が並列作業中で一時的にコンパイルできない場合、既存の `target/wasm32-unknown-unknown/release/streamdown.wasm` が semantic fence 対応済みなら次でも起動できます。

```sh
SKIP_BUILD=1 ./studio/build.sh
python3 -m http.server 8080 --directory studio
```

## UI extension

既存の `:::llm` semantic fence を利用します。専用 AST variant はまだ増やしていないため、MDA1 の互換性を維持したまま Generative UI を試せます。

```md
:::llm ui type=chart id=latency title="Streaming latency"
values=58,51,47,40,36,31,28
unit=ms
:::
```

現在の component:

- `metric`: label/value/unit/trend
- `chart`: title/values/unit
- `slider`: state/min/max/value/step/unit
- `button`: label/action
- `progress`: state/min/max/unit
- `layout`: title/columns/gap/min。直後の連続したUI componentをgrid化し、component側の`span`で列結合

`slider` と `button` は runtime state を共有します。Markdown 内の `{{stateName}}` は同じ state を参照します。

```md
Temperature: {{temperature}}°C

:::llm ui type=slider state=temperature min=0 max=100 value=42
label=Temperature
unit=°C
:::

:::llm ui type=button
label=+5°C
action=increment:temperature:5
:::
```

`button.action` は現時点で `increment:key:n`, `decrement:key:n`, `set:key:value` のみを許可します。任意 JavaScript や HTML は実行しません。


### Generated layout

`layout` は後続する連続 UI block をまとめます。通常 Markdown が現れた時点で layout は終了します。layout 自体も semantic fence なので、`columns=` が生成途中でも構造を更新できます。

```md
:::llm ui type=layout id=dashboard
columns=2
gap=14
min=220
title=Live dashboard
:::

:::llm ui type=metric id=throughput
label=Throughput
value=2.4M
:::

:::llm ui type=chart id=latency span=2
values=58,51,47,40,36
unit=ms
:::
```

`columns` は1〜4、`gap` は0〜40px、`min` は120〜480pxへ制限されます。LLM出力をそのまま無制限なCSS値として通しません。

`progress` は同名 state に自動追従するため、slider/button/本文bindingと相互作用します。

## Streaming behavior

UI fence は通常の code block と同じ `SpliceCode` delta を利用します。したがって巨大な body を毎回再送せず、現在行の差分だけで component を更新できます。たとえば chart の `values=` 行が生成途中なら、受信済みの数値だけで chart を描画し続けます。
