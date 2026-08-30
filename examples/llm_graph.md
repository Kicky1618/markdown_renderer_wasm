# LLM dependency graph example

Search first:

:::llm tool id=search name="web search"
{"query":"streaming markdown wasm"}
:::

Build a summary from that result:

:::llm artifact id=summary mime=application/json depends=tool:search
{"title":"Streamdown","fast":true}
:::

Render a metric from the artifact:

:::llm ui id=metric type=metric depends=artifact:summary
{"label":"Fast path","value":1,"unit":"enabled"}
:::

The final paragraph can refer to @[artifact:summary] and @[ui:metric].
