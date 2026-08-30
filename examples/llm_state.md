# Streaming semantic state

Initialize shared state:

:::llm state id=session
{"count":0,"status":"warming","nested":{"a":1}}
:::

Apply the first merge patch after initialization:

:::llm patch id=step1 target=state:session depends=state:session if_revision=1
{"count":1,"status":"ready","nested":{"b":2}}
:::

Apply another patch after the first one so updates stay deterministic:

:::llm patch id=step2 target=state:session depends=patch:step1 if_revision=2
{"nested":{"a":null},"extra":true}
:::

The final state can be referred to as @[state:session], while the latest update node is @[patch:step2].
