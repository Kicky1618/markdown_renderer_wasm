const MAX_REQUESTED_PACKS = 128;

const aliases = new Map([
  ["aarch64asm", "assembly"],
  ["asm", "assembly"],
  ["assembly", "assembly"],
  ["astro", "html"],
  ["bash", "shell"],
  ["c", "cpp"],
  ["c#", "csharp"],
  ["c++", "cpp"],
  ["cairo", "cairo"],
  ["cairo1", "cairo"],
  ["cc", "cpp"],
  ["cfg", "ini"],
  ["cjs", "javascript"],
  ["cl", "lisp"],
  ["clc", "opencl"],
  ["clj", "clojure"],
  ["cljc", "clojure"],
  ["cljs", "clojure"],
  ["clojure", "clojure"],
  ["common-lisp", "lisp"],
  ["commonlisp", "lisp"],
  ["commonmark", "markdown"],
  ["comp", "glsl"],
  ["conf", "ini"],
  ["containerfile", "dockerfile"],
  ["cpp", "cpp"],
  ["cs", "csharp"],
  ["csharp", "csharp"],
  ["css", "css"],
  ["cts", "javascript"],
  ["cu", "cuda"],
  ["cuda", "cuda"],
  ["cuh", "cuda"],
  ["cxx", "cpp"],
  ["d", "dlang"],
  ["dart", "dart"],
  ["dlang", "dlang"],
  ["docker", "dockerfile"],
  ["dockerfile", "dockerfile"],
  ["dotnet", "csharp"],
  ["edn", "clojure"],
  ["elixir", "elixir"],
  ["erl", "erlang"],
  ["erlang", "erlang"],
  ["ex", "elixir"],
  ["exs", "elixir"],
  ["f#", "fsharp"],
  ["f03", "fortran"],
  ["f08", "fortran"],
  ["f77", "fortran"],
  ["f90", "fortran"],
  ["f95", "fortran"],
  ["fortran", "fortran"],
  ["frag", "glsl"],
  ["fs", "fsharp"],
  ["fsharp", "fsharp"],
  ["fsx", "fsharp"],
  ["fx", "hlsl"],
  ["gas", "assembly"],
  ["geom", "glsl"],
  ["glsl", "glsl"],
  ["gnumake", "makefile"],
  ["go", "go"],
  ["golang", "go"],
  ["gql", "graphql"],
  ["gradle", "groovy"],
  ["graphql", "graphql"],
  ["groovy", "groovy"],
  ["h", "cpp"],
  ["haskell", "haskell"],
  ["hcl", "terraform"],
  ["hlsl", "hlsl"],
  ["hpp", "cpp"],
  ["hrl", "erlang"],
  ["hs", "haskell"],
  ["htm", "html"],
  ["html", "html"],
  ["ini", "ini"],
  ["java", "java"],
  ["javascript", "javascript"],
  ["jl", "julia"],
  ["js", "javascript"],
  ["json", "json"],
  ["jsonc", "json"],
  ["jsx", "javascript"],
  ["julia", "julia"],
  ["kotlin", "kotlin"],
  ["kt", "kotlin"],
  ["kts", "kotlin"],
  ["lhs", "haskell"],
  ["lisp", "lisp"],
  ["lua", "lua"],
  ["m", "matlab"],
  ["make", "makefile"],
  ["makefile", "makefile"],
  ["markdown", "markdown"],
  ["matlab", "matlab"],
  ["md", "markdown"],
  ["mdx", "markdown"],
  ["mjs", "javascript"],
  ["ml", "ocaml"],
  ["mli", "ocaml"],
  ["mm", "objectivec"],
  ["move", "move"],
  ["movelang", "move"],
  ["mts", "javascript"],
  ["nasm", "assembly"],
  ["nim", "nim"],
  ["nimrod", "nim"],
  ["node", "javascript"],
  ["objc", "objectivec"],
  ["objective-c", "objectivec"],
  ["objectivec", "objectivec"],
  ["ocaml", "ocaml"],
  ["octave", "matlab"],
  ["opencl", "opencl"],
  ["perl", "perl"],
  ["perl5", "perl"],
  ["php", "php"],
  ["php8", "php"],
  ["pl", "perl"],
  ["plg", "prolog"],
  ["plist", "xml"],
  ["pm", "perl"],
  ["postgres", "sql"],
  ["postgresql", "sql"],
  ["powershell", "powershell"],
  ["prolog", "prolog"],
  ["properties", "ini"],
  ["proto", "protobuf"],
  ["proto3", "protobuf"],
  ["protobuf", "protobuf"],
  ["ps1", "powershell"],
  ["pwsh", "powershell"],
  ["py", "python"],
  ["python", "python"],
  ["python3", "python"],
  ["r", "r"],
  ["racket", "scheme"],
  ["rb", "ruby"],
  ["rkt", "scheme"],
  ["rlang", "r"],
  ["rs", "rust"],
  ["rscript", "r"],
  ["ruby", "ruby"],
  ["rust", "rust"],
  ["sc", "scala"],
  ["scala", "scala"],
  ["scheme", "scheme"],
  ["scm", "scheme"],
  ["scss", "css"],
  ["sh", "shell"],
  ["shader", "glsl"],
  ["shaderlab", "hlsl"],
  ["shell", "shell"],
  ["sol", "solidity"],
  ["solidity", "solidity"],
  ["sql", "sql"],
  ["sv", "verilog"],
  ["svelte", "html"],
  ["svg", "xml"],
  ["svh", "verilog"],
  ["swift", "swift"],
  ["systemverilog", "verilog"],
  ["terraform", "terraform"],
  ["tf", "terraform"],
  ["toml", "toml"],
  ["ts", "javascript"],
  ["tsx", "javascript"],
  ["typescript", "javascript"],
  ["v", "verilog"],
  ["vala", "vala"],
  ["vapi", "vala"],
  ["vb", "vbnet"],
  ["vbnet", "vbnet"],
  ["verilog", "verilog"],
  ["vert", "glsl"],
  ["vhd", "vhdl"],
  ["vhdl", "vhdl"],
  ["visualbasic", "vbnet"],
  ["vue", "html"],
  ["webgpu", "wgsl"],
  ["wgsl", "wgsl"],
  ["x86_64asm", "assembly"],
  ["x86asm", "assembly"],
  ["xhtml", "html"],
  ["xml", "xml"],
  ["xsd", "xml"],
  ["xsl", "xml"],
  ["xslt", "xml"],
  ["yaml", "yaml"],
  ["yml", "yaml"],
  ["zig", "zig"],
  ["zsh", "shell"],
]);

const requested = new Set();
const registered = new Set();

function resolvePack(name) {
  const normalized = String(name).trim().toLowerCase();
  const mapped = aliases.get(normalized);
  if (mapped) return mapped;
  return normalized.length > 0 && normalized.length <= 48 && /^[a-z0-9_-]+$/.test(normalized)
    ? normalized
    : null;
}

function invalidateRenderer() {
  const canvas = document.getElementById("app");
  if (canvas instanceof HTMLCanvasElement) canvas.width = Math.min(0xffffffff, canvas.width + 1);
}

export async function loadLanguagePack(name) {
  const pack = resolvePack(name);
  if (!pack || requested.has(pack) || requested.size >= MAX_REQUESTED_PACKS) return false;
  requested.add(pack);
  try {
    const url = new URL(`./langpacks/${pack}.slp`, import.meta.url);
    const response = await fetch(url, { cache: "force-cache" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const binary = new Uint8Array(await response.arrayBuffer());
    const wasm = await import("./pkg/streamdown_web.js");
    const registeredNow = wasm.register_language_pack_binary(binary);
    if (registeredNow) {
      registered.add(pack);
      const root = document.documentElement;
      root.dataset.languagePackRegistered = pack;
      root.dataset.languagePackRegisteredCount = String(registered.size);
      root.dataset.languagePacks = [...registered].sort().join(",");
      invalidateRenderer();
    } else {
      document.documentElement.dataset.languagePackError = `${pack}:wasm-rejected`;
    }
    return registeredNow;
  } catch (error) {
    document.documentElement.dataset.languagePackError = `${pack}:${String(error)}`;
    console.warn(`language pack ${JSON.stringify(pack)} unavailable:`, error);
    return false;
  }
}

export const __test = { resolvePack };
