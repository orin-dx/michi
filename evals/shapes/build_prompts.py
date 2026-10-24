#!/usr/bin/env python3
"""Build prompts/all.jsonl: one line per (question, format) cell listed in renders/manifest.json.

The prompt is the TOON pilot's template. Unlike the pilot, no format is described: the data is introduced by its
format name only ("TOON (Token-Oriented Object Notation)", "JSON", "Markdown"...), so every format is cold-read. The
pilot gave TOON a one-line structure description and Markdown-KV another; this eval adds none, for any format.
"""
import json
import os

ROOT = os.path.dirname(os.path.abspath(__file__))
EXT = {"toon": "toon", "toon_ref": "toon", "json_pretty": "json", "json_min": "json", "json_min_omit": "json",
       "yaml": "yaml", "toml": "toml", "xml": "xml", "md": "md"}
NAME = {"toon": "TOON (Token-Oriented Object Notation)", "toon_ref": "TOON (Token-Oriented Object Notation)",
        "json_pretty": "JSON", "json_min": "JSON", "json_min_omit": "JSON", "yaml": "YAML", "toml": "TOML", "xml": "XML",
        "md": "Markdown"}
PROMPT = """You are given the output of a tool call, in {fmt_name} format.

<data>
{context}
</data>

Question: {question}

Reply with only the answer, no explanation. If the answer is a list, give the values separated by commas. \
If the value is missing or null, reply null. Give numbers exactly as they appear in the data."""


def main():
    manifest = json.load(open(os.path.join(ROOT, "renders", "manifest.json"), encoding="utf-8"))
    questions = [json.loads(l) for l in open(os.path.join(ROOT, "questions.jsonl"), encoding="utf-8")]
    os.makedirs(os.path.join(ROOT, "prompts"), exist_ok=True)
    n = 0
    with open(os.path.join(ROOT, "prompts", "all.jsonl"), "w", encoding="utf-8") as out:
        for q in questions:
            for fmt in manifest["cells"][q["shape"]]:
                path = os.path.join(ROOT, "renders", fmt, "%s.%s" % (q["shape"], EXT[fmt]))
                context = open(path, encoding="utf-8").read()
                out.write(json.dumps({
                    "id": q["id"], "shape": q["shape"], "format": fmt, "kind": q["kind"], "expected": q["answer"],
                    "answer_type": q["answer_type"],
                    "prompt": PROMPT.format(fmt_name=NAME[fmt], context=context, question=q["question"]),
                }, ensure_ascii=False) + "\n")
                n += 1
    print("wrote %d prompts" % n)


if __name__ == "__main__":
    main()
