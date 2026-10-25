// Render every corpus/<shape>.json with the official TOON reference encoder (@toon-format/toon 4.4.0) into
// renders/toon_ref/<shape>.toon, and check that decode(encode(x)) gives back the source. These cells are marked
// "reference encoder, not michi" in the results.
import { readFileSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
// The encoder is the pinned devDependency of packages/michi-node (run `pnpm install` there). This folder has no
// package.json of its own, so Callisto does not mistake it for a package.
const { encode, decode } = createRequire(join(here, "..", "..", "packages", "michi-node", "package.json"))(
  "@toon-format/toon",
);
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
