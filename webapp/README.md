# Canvas renderer demo

The document is rendered entirely by `wgpu` into one canvas. DOM elements are
not created for Markdown nodes. The only HTML rendering element is the canvas
required to obtain a WebGPU surface.

```sh
./webapp/build.sh
python3 -m http.server 8080 --directory webapp
```

Open `http://localhost:8080` in a WebGPU-capable browser. Use the mouse wheel or
trackpad to scroll.

The renderer automatically falls back from WebGPU to WebGL2 and then Canvas2D.
Use `?renderer=webgpu`, `?renderer=webgl`, or `?renderer=canvas2d` to choose the
initial backend explicitly. WebGL2 shares the full AST/layout/highlighting path
with WebGPU. Canvas2D uses the same parser AST and supports math, highlighted
code, lists, tables, scrolling, text selection/copy, runtime font size, and the
streaming fade.

GPU backend initialization has a 5-second watchdog per candidate. A browser or
driver that exposes WebGPU/WebGL2 but never finishes adapter/device creation is
therefore downgraded instead of leaving the viewer stuck on a blank canvas. The
active canvas exposes `data-renderer`, `data-renderer-requested`, and
`data-renderer-fallback-depth` for diagnostics.

Run the browser-level compatibility smoke test with:

```sh
./webapp/tests/browser_smoke.sh
```

It launches a local Chrome/Chromium when available, verifies forced Canvas2D,
forced WebGL2 with allowed fallback, and automatic backend selection, then sends
keyboard commands to the final canvas. This specifically catches regressions
where a failed GPU surface is replaced but input listeners remain attached to
the discarded canvas.

Renderer compatibility is centralized in `src/compat.rs`. `renderer=auto` and
`renderer=webgpu` use WebGPU → WebGL2 → Canvas2D; `renderer=webgl` never upgrades
to WebGPU and uses WebGL2 → Canvas2D; `renderer=canvas2d` stays on Canvas2D.
Both GPU paths request the WebGL2 downlevel wgpu limits so the same WGSL,
instance layout, and bind groups are portable between them. Backend-specific
performance policy is kept out of layout code: WebGPU preserves full 8-bit glyph
coverage, while WebGL2 quantizes coverage runs and limits expensive scene
rebuilds to about 30 Hz while still presenting scroll-only frames at the browser
animation rate. The active canvas exposes `data-renderer-requested`,
`data-renderer`, `data-renderer-fallback-depth`, and `data-renderer-api` for
diagnostics and automated browser tests.

TPS and document length are URL parameters. Defaults are 50,000 TPS and 250
copies of the mock document:

```text
http://localhost:8080/?tps=1000000&repeat=10000&autoscroll=1
```

`tps` accepts 1–10,000,000 and `repeat` accepts 1–100,000. High-TPS tokens are
batched once per animation frame. Only blocks around the viewport are converted
to GPU vertices, so a very long document does not create a giant draw call.

Auto-scroll is enabled by default. Scrolling upward pauses it; returning to the
bottom resumes it. Use `autoscroll=0` to start with manual scrolling.

The responsive control dock can pause/resume streaming, toggle automatic
following, jump through the document, change text size, copy the full source,
and enter fullscreen. The same actions are keyboard-accessible: `P` toggles
streaming, `A` toggles following, `+`/`-` changes text size, and navigation uses
the arrow, Page Up/Down, Home, End, and Space keys. The status pill mirrors the
active renderer and current viewer state.

Document search opens with `Ctrl+F`/`Cmd+F`. It uses a case-insensitive Rust
trie with indexed character suffixes, providing fast partial-word lookup without
rescanning the document for every keystroke. Matches are highlighted in yellow,
the active match in orange, and Enter/Shift+Enter moves forward/backward.

Use the `fontsize=16` and `fade=180` URL parameters to adjust the rendering.
Use `doc=easy` to render
`easy_test.md` or `doc=stress` to render `math_stress_test.md`.
Use `doc=code` to render the code highlighting test document.

A GPU-drawn scrollbar is shown at the right edge. It can be clicked and dragged
without creating an HTML scrollbar or document DOM.

Fenced code uses the same allocation-free, Tree-sitter-inspired
scan/shift/recover/capture parser while a fence is streaming and after it closes.
Incomplete strings, comments, and delimiter stacks are recovered provisionally;
multiline comments and contextual function, type, macro, and operator
classification therefore remain active during streaming. Rust,
JavaScript/TypeScript, Python, shell, JSON, C/C++, Java, and Go keyword sets are
built in; unknown languages keep neutral text with generic literals/comments.
Only lines around the viewport are converted to GPU instances, including when a
single fenced block contains thousands of lines.

Language support is declared in `src/languages.rs`. Each `LanguageProfile`
keeps fence aliases, keywords, built-in types, declaration captures, and scanner
features together; adding a conventional language normally requires one profile
and one entry in `LANGUAGES`. Registry tests reject duplicate aliases and
declaration words missing from the corresponding keyword set.

Macro captures cover Rust bang macros, attributes, declarations, and `$`
metavariables; C/C++ directives, defined names, header probes, and conventional
upper-case references; and Python decorators. Directive bodies are parsed
normally instead of being flattened into one macro-colored line, and incomplete
streaming constructs use the same recovery path as other syntax.

Math spans such as `$E=mc^2$`, `$$\frac{a}{b}$$`, `\sqrt{x}`, and Greek
commands are parsed and typeset by [RaTeX](https://github.com/erweixin/RaTeX).
Its embedded KaTeX fonts rasterize each display list to a transparent PNG,
which is decoded to RGBA and cached before being added to the GPU scene. Hold
Shift while using the wheel to horizontally scroll an overflowing display
formula. RaTeX is distributed under the MIT license.

Body text uses the embedded Noto Sans CJK JP TTF subset, while fenced code uses
Noto Sans Mono with Noto Sans CJK JP as its fallback. Cached outline rasterization
provides antialiased coverage for the GPU renderers. The fallback includes JIS X
0208 Japanese, Latin, Greek, Cyrillic, and common symbols. Missing codepoints fall
back to U+FFFD instead of disappearing. Canvas2D loads the same files as web fonts.
Glyph positions remain integer-snapped, while the rasterized outline coverage carries
the antialiasing, so scrolling does not blur the text.

## LLM semantic Markdown and Generative UI

The core parser keeps LLM-specific syntax compatible with ordinary Markdown
renderers. Semantic constructs are normalized onto existing AST nodes instead
of requiring a second parser or a JSON side channel:

```md
Fact [[cite:doc-42|design spec]] is reflected in @[artifact:plot-1].

:::llm tool name=search id=q1
{"query":"streaming markdown wasm"}
:::

:::llm ui type=metric label="Tokens / second" value=42000
:::
```

`[[cite:source|label]]` becomes a normal link with an `llm:cite:` destination,
`@[kind:id]` becomes a link with an `llm:<kind>:<id>` destination, and
`:::llm ...` becomes a code block whose language starts with `llm:`. This means
the normal Viewer can still render the document when it does not opt into the
extra semantics. Open semantic fences stream through the same tail-splice path
as code fences, so a large tool result or generated artifact is not retransmitted
on every token.

The Generative UI page at `/generative/` interprets only the allowlisted
`:::llm ui` components documented in `generative/README.md`. Unknown kinds,
unknown components, malformed attributes, and ordinary Markdown remain data;
they are never evaluated as JavaScript or injected as arbitrary HTML. Actions
are restricted to the built-in state operations, expressions use the dedicated
safe evaluator, and the canvas component accepts only its fixed drawing DSL.

A useful local verification loop is:

```sh
./webapp/build.sh
npm test
node webapp/generative/tests.mjs
python3 -m http.server 8080 --directory webapp
```

Then compare both views of the same streaming protocol:

- `http://localhost:8080/` — canvas Markdown viewer with WebGPU → WebGL2 → Canvas2D fallback.
- `http://localhost:8080/generative/` — semantic `:::llm ui` promotion.

For parser-level syntax and wire-format details, see `../LLM_EXTENSIONS.md` and
`../FORMAT.md`. The command-line semantic inspection tools live in `../tools/`.

