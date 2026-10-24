// Render every corpus/<shape>.json with the official TOON reference encoder (@toon-format/toon 4.4.0) into
// renders/toon_ref/<shape>.toon, and check that decode(encode(x)) gives back the source. These cells are marked
// "reference encoder, not michi" in the results.
import { encode, decode } from "@toon-format/toon";
import { readFileSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "renders", "toon_ref");
mkdirSync(out, { recursive: true });
let bad = 0;
for (const f of readdirSync(join(here, "corpus")).filter((f) => f.endsWith(".json")).sort()) {
  const doc = JSON.parse(readFileSync(join(here, "corpus", f), "utf8"));
  const value = doc.wrapper ? { [doc.wrapper]: doc.data } : doc.data;
  const text = encode(value);
  if (!isDeepStrictEqual(decode(text), value)) {
    console.error("round-trip mismatch:", f);
    bad++;
  }
  writeFileSync(join(out, f.replace(/\.json$/, ".toon")), text);
}
if (bad) process.exit(1);
console.log("toon_ref: rendered and round-tripped all shapes");
