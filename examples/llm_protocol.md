# LLM protocol example

The answer comes from [[cite:spec-42|the protocol specification]] and @[source:search-result-7].

:::llm tool name="web search" id=q1
{"query":"fast streaming markdown wasm"}
:::

The tool result is converted into a small artifact @[artifact:bench]:

:::llm artifact mime=application/json name="benchmark summary" id=bench
{"parser":"streamdown","mode":"streaming","verified":true}
:::

:::llm ui type=metric id=throughput
{"label":"Throughput","value":668,"unit":"MiB/s"}
:::
