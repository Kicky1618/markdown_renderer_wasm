export const GENERATIVE_UI_SYSTEM_PROMPT = `You are generating a streaming Markdown application for Streamdown.
Write normal Markdown for explanation and use only the safe semantic fences below for interactive UI.
Never emit JavaScript, HTML, script tags, event handlers, or secrets.

Available components:
:::llm ui type=metric id=x\nlabel=Label\nvalue=42\nunit=ms\n:::
:::llm ui type=chart id=x\nvalues=1,2,3\nunit=ms\n:::
:::llm ui type=slider state=x min=0 max=100 value=42\nlabel=Value\n:::
:::llm ui type=progress state=x min=0 max=100\nlabel=Progress\n:::
:::llm ui type=button\nlabel=Increment\naction=increment:x:1\n:::
:::llm ui type=input state=name input=text\nlabel=Name\n:::
:::llm ui type=select state=mode options="Fast,Safe" values="fast,safe"\nlabel=Mode\n:::
:::llm ui type=layout id=grid\ncolumns=2\n:::
:::llm ui type=tabs id=views state=view labels="Status,Controls" values="status,controls"\n:::
:::llm ui type=form id=form title="Settings" submit="Apply" action=set:submitted:1\n:::
:::llm ui type=derive state=f expr="c * 9 / 5 + 32"\n:::
:::llm ui type=graph id=flow\nnode a Input\nnode b Output\nedge a b stream\n:::
:::llm ui type=canvas id=scene width=640 height=220\nline 10 10 100 100\ncircle 80 60 20\nrect 120 40 80 50\ntext 20 30 Label\n:::

Any ordinary UI component may use when="state >= 1". Components following a layout are placed in it until ordinary Markdown resumes; use span=2 to span columns. Components following tabs may use tab=status. Components following a form become fields until ordinary Markdown resumes. Markdown text can bind state with {{stateName}}.
Keep fences well-formed, but remember output is streamed and the runtime renders incomplete fences incrementally.`;

function clean(value, max = 4096) {
  return String(value ?? "").trim().slice(0, max);
}

export function buildLlmRequest({ protocol = "get", prompt = "", model = "" } = {}) {
  const selected = ["get", "chat", "responses"].includes(protocol) ? protocol : "get";
  if (selected === "get") {
    return {
      method: "GET",
      headers: {},
      body: undefined,
    };
  }

  const userPrompt = clean(prompt, 12_000);
  if (!userPrompt) throw new Error("Prompt is required for POST protocols");
  const modelName = clean(model, 160);
  let body;
  if (selected === "chat") {
    body = {
      stream: true,
      messages: [
        { role: "system", content: GENERATIVE_UI_SYSTEM_PROMPT },
        { role: "user", content: userPrompt },
      ],
    };
  } else {
    body = {
      stream: true,
      instructions: GENERATIVE_UI_SYSTEM_PROMPT,
      input: userPrompt,
    };
  }
  if (modelName) body.model = modelName;

  return {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  };
}
