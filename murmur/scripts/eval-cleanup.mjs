// Runs the AI clean-up prompt against sample transcripts through a local Ollama
// model and prints a side-by-side report. Use it to compare models or prompt tweaks.
//
//   ollama pull qwen2.5:3b
//   npm run eval:cleanup -- qwen2.5:3b
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const model = process.argv[2] || "qwen2.5:3b";
const url = process.env.OLLAMA_URL || "http://127.0.0.1:11434";
const base = fs.readFileSync(path.join(root, "src-tauri/src/prompts/cleanup.md"), "utf8").trim();
const cases = JSON.parse(fs.readFileSync(path.join(root, "scripts/cleanup-cases.json"), "utf8"));

// Keep in sync with style_instructions() in src-tauri/src/text.rs.
const STYLE = {
  neutral: "Style: clear, natural written text with standard punctuation.",
  casual: "Style: casual chat message. Keep it relaxed; contractions are fine; a trailing period on a one-line message is optional.",
  formal: "Style: professional email or document. Use complete sentences and proper paragraphs. Do not add a greeting or sign-off unless spoken.",
  code: "Style: the destination is a code editor or terminal. Be literal.",
};

const norm = (s) => s.toLowerCase().replace(/[^\p{L}\p{N}\s]/gu, "").replace(/\s+/g, " ").trim();
function similarity(a, b) {
  const A = new Set(norm(a).split(" ")), B = new Set(norm(b).split(" "));
  const inter = [...A].filter((w) => B.has(w)).length;
  return inter / Math.max(1, new Set([...A, ...B]).size);
}

let total = 0;
for (const c of cases) {
  const t0 = Date.now();
  const res = await fetch(`${url}/api/chat`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model, stream: false, options: { temperature: 0 },
      messages: [
        { role: "system", content: `${base}\n\n${STYLE[c.style] ?? STYLE.neutral}` },
        { role: "user", content: `<transcript>\n${c.input}\n</transcript>` },
      ],
    }),
  }).then((r) => r.json());
  const out = (res.message?.content ?? "").replace(/<think>[\s\S]*?<\/think>/g, "").trim();
  const score = similarity(out, c.expected);
  total += score;
  console.log(`\n[${c.style}] ${Date.now() - t0} ms  word overlap ${(score * 100).toFixed(0)}%`);
  console.log(`  heard:    ${c.input}`);
  console.log(`  expected: ${c.expected.replace(/\n/g, " / ")}`);
  console.log(`  got:      ${out.replace(/\n/g, " / ")}`);
}
console.log(`\n${model}: average word overlap ${((total / cases.length) * 100).toFixed(0)}% across ${cases.length} cases`);
