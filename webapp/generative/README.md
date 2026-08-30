# Generative UI mode

`webapp/` に統合された Streamdown の Generative UI モードです。

通常の Viewer は `../`、Generative UI はこのディレクトリから開きます。`../build.sh` は既存 renderer WASM と core parser WASM の両方を生成し、Generative UI が使う `streamdown.wasm` / `streamdown.js` をここへ配置します。

対応する `:::llm ui` component:

- `layout`, `tabs`, `form`
- `metric`, `chart`, `canvas`, `progress`
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
