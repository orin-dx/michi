// MCP conformance: every `CallToolResult` michi assembles, validated by the official MCP TypeScript SDK's own schema
// (@modelcontextprotocol/sdk, a dev dependency of this package only; no Rust crate depends on it). In July an
// independent pass found two wire-format bugs (a missing `"type":"text"`, a flat `audience` instead of
// `annotations.audience[]`) that michi's own tests missed because they only compared michi with itself. This file is
// that independent check, made permanent. Nothing here reads michi's expectations: the SDK decides what is valid.
//
// What the SDK schema is strict about: `CallToolResultSchema` is a *loose* object and the content-block schemas strip
// unknown keys, so a plain `safeParse` accepts misspelled or extra fields. To close that gap each result is also
// checked against `.strict()` variants of the SDK's own schemas, and the parsed value must equal the input (a stripped
// key would show up as a difference).
import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import {
  CallToolResultSchema,
  TextContentSchema,
  AnnotationsSchema,
  LATEST_PROTOCOL_VERSION,
} from '@modelcontextprotocol/sdk/types.js'
import {
  AgentResponse,
  emptyState,
  emptyStateWithHints,
  renderDomainError,
  renderStatus,
  renderPipeline,
  renderPartialSuccess,
  renderRecovery,
} from '../index.js'

const s = (strVal) => ({ type: 'str', strVal })
const i = (intVal) => ({ type: 'int', intVal })
const f = (floatVal) => ({ type: 'float', floatVal })
const b = (boolVal) => ({ type: 'bool', boolVal })
const nul = { type: 'null' }

const StrictResult = CallToolResultSchema.strict()
const StrictText = TextContentSchema.strict()
const StrictAnnotations = AnnotationsSchema.strict()

/** The exact keys the wire shape allows, asserted by hand in case the SDK schema ever loosens. */
const RESULT_KEYS = ['content', 'isError', 'structuredContent']
const BLOCK_KEYS = ['annotations', 'text', 'type']

const issuesOf = (error) => JSON.stringify(error.issues, null, 2)

/** Fails with the SDK's issue list (and the offending value) unless `result` is a valid MCP `CallToolResult`. */
const assertConforms = (result, label) => {
  const where = `${label}\nresult: ${JSON.stringify(result)}`

  const loose = CallToolResultSchema.safeParse(result)
  assert.ok(
    loose.success,
    `SDK CallToolResultSchema rejected it: ${label}\n${loose.success ? '' : issuesOf(loose.error)}\n${where}`,
  )

  const strict = StrictResult.safeParse(result)
  assert.ok(
    strict.success,
    `strict CallToolResultSchema rejected it: ${label}\n${strict.success ? '' : issuesOf(strict.error)}\n${where}`,
  )

  assert.deepEqual(Object.keys(result).sort(), RESULT_KEYS, `unexpected result keys: ${where}`)
  assert.ok(result.content.length >= 1, `content must not be empty: ${where}`)
  for (const [n, block] of result.content.entries()) {
    const text = StrictText.safeParse(block)
    assert.ok(
      text.success,
      `block ${n} is not a strict MCP TextContent: ${label}\n${text.success ? '' : issuesOf(text.error)}\n${where}`,
    )
    const ann = StrictAnnotations.safeParse(block.annotations)
    assert.ok(ann.success, `block ${n} annotations rejected: ${label}\n${ann.success ? '' : issuesOf(ann.error)}\n${where}`)
    assert.deepEqual(Object.keys(block).sort(), BLOCK_KEYS, `block ${n} has unexpected keys: ${where}`)
    assert.equal(block.type, 'text', where)
    assert.ok(
      Array.isArray(block.annotations.audience) && block.annotations.audience.length >= 1,
      `audience must be a non-empty array: ${where}`,
    )
  }

  // The per-block checks above name a problem first; this catches any key the SDK parse stripped or defaulted.
  assert.deepEqual(strict.data, result, `the SDK parse changed the value (stripped or defaulted a key): ${where}`)

  // Block 0 is always the assistant's; a user block, when present, is second.
  assert.deepEqual(result.content[0].annotations.audience, ['assistant'], where)
  if (result.content.length > 1) {
    assert.equal(result.content.length, 2, where)
    assert.deepEqual(result.content[1].annotations.audience, ['user'], where)
  }

  assert.equal(typeof result.isError, 'boolean', where)
  assert.ok(
    typeof result.structuredContent === 'object' &&
      result.structuredContent !== null &&
      !Array.isArray(result.structuredContent),
    `structuredContent must be a JSON object: ${where}`,
  )

  // It must survive the wire: JSON in, JSON out, unchanged.
  assert.deepEqual(JSON.parse(JSON.stringify(result)), result, `changed across a JSON round trip: ${where}`)
}

/** Builds a response with `setup`, converts it through the shipped binary, and checks the result conforms. */
const check = (label, setup) => {
  const r = new AgentResponse('thing')
  setup(r)
  const result = r.toCallToolResult()
  assertConforms(result, label)
  return result
}

const rows = (...cells) => cells.map((row) => row.map(s))

void describe(`MCP CallToolResult conformance (SDK protocol ${LATEST_PROTOCOL_VERSION} schema)`, () => {
  void it('the checker itself rejects the July wire bugs and stray keys', () => {
    const good = {
      content: [{ type: 'text', text: 'x', annotations: { audience: ['assistant'] } }],
      isError: false,
      structuredContent: {},
    }
    assertConforms(good, 'baseline')
    const bad = {
      'missing type': { ...good, content: [{ text: 'x', annotations: { audience: ['assistant'] } }] },
      'flat audience': { ...good, content: [{ type: 'text', text: 'x', audience: ['assistant'] }] },
      'audience not an array': { ...good, content: [{ type: 'text', text: 'x', annotations: { audience: 'assistant' } }] },
      'unknown role': { ...good, content: [{ type: 'text', text: 'x', annotations: { audience: ['robot'] } }] },
      'misspelled key': { ...good, content: [{ type: 'text', text: 'x', annotation: { audience: ['assistant'] } }] },
      'extra annotation key': {
        ...good,
        content: [{ type: 'text', text: 'x', annotations: { audience: ['assistant'], aud: [] } }],
      },
      'extra result key': { ...good, is_error: true },
      'snake_case isError': { content: good.content, is_error: false, structuredContent: {} },
      'structuredContent as string': { ...good, structuredContent: '{}' },
      'isError as string': { ...good, isError: 'false' },
    }
    for (const [name, value] of Object.entries(bad)) {
      assert.throws(() => assertConforms(value, name), `the checker accepted: ${name}`)
    }
  })

  void it('assistant block only', () => {
    const result = check('assistant only', (r) => r.items(rows(['a', 'b']), ['x', 'y']))
    assert.equal(result.content.length, 1)
  })

  void it('assistant and user blocks together', () => {
    const result = check('assistant + user', (r) => {
      r.items(rows(['a']), ['x'])
      r.humanContent('Two results found.')
    })
    assert.equal(result.content.length, 2)
    assert.equal(result.content[1].text, 'Two results found.')
  })

  void it('a fresh builder with nothing set', () => {
    check('fresh builder', () => {})
  })

  void it('error results, plain and with human content', () => {
    const plain = check('error', (r) => {
      r.items(rows(['boom']), ['message'])
      r.asError()
    })
    assert.equal(plain.isError, true)
    const both = check('error + human', (r) => {
      r.items(rows(['boom']), ['message'])
      r.asError()
      r.humanContent('Something went wrong.')
    })
    assert.equal(both.isError, true)
  })

  void it('DomainError-rendered text as the body of an error result', () => {
    for (const [code, message, hints, causes] of [
      ['not_found', 'No such repo', ['Check the name'], undefined],
      ['rate_limited', 'Slow down: "quota"\nretry in 30s', ['wait', 'retry, later'], ['upstream 429', 'timeout']],
      ['invalid_input', '日本語 🙂 é', [], []],
    ]) {
      const text = renderDomainError(code, message, hints, true, causes)
      const result = check(`domain error ${String(code)}`, (r) => {
        r.asError()
        r.humanContent(text)
      })
      assert.equal(result.isError, true)
      assert.equal(result.content[1].text, text)
    }
  })

  void it('structuredContent is present, an object, and carries the response fields', () => {
    const result = check('structured', (r) => {
      r.items(rows(['a'], ['b']), ['v'])
      r.hint('next: page=2')
      r.recoveryHint('search', 'try a broader query')
    })
    assert.equal(typeof result.structuredContent.body, 'string')
    assert.deepEqual(result.structuredContent.hints, ['next: page=2'])
    assert.equal(result.structuredContent.isError, false)
    assert.ok(Array.isArray(result.structuredContent.recovery))
  })

  void it('hints and recovery hints', () => {
    check('hints', (r) => {
      r.items(rows(['a']), ['v'])
      for (const h of ['one', 'two, with comma', 'three "quoted"', 'multi\nline']) r.hint(h)
    })
    check('recovery', (r) => {
      r.items(rows(['a']), ['v'])
      r.recoveryHint('retry')
      r.recoveryHint('search', 'broaden the query')
    })
    check('hints + recovery + error', (r) => {
      r.items(rows(['a']), ['v'])
      r.hint('h')
      r.recoveryHint('t', 'because')
      r.asError()
    })
  })

  void it('empty states', () => {
    check('no rows', (r) => r.items([], ['a', 'b']))
    check('emptyState as human content', (r) => {
      r.items([], ['a'])
      r.humanContent(emptyState('issue'))
    })
    check('emptyStateWithHints as human content', (r) => {
      r.items([], ['a'])
      r.humanContent(emptyStateWithHints('issue', ['try a wider range', 'a, b']))
    })
    check('kv with no items', (r) => r.kvItems([]))
  })

  void it('truncated lists and truncated cells', () => {
    check('count cut', (r) => {
      r.items(rows(['a'], ['b']), ['v'])
      r.totalCount(5000)
      r.hint('showing 2 of 5000')
    })
    const result = check('cell cut', (r) => {
      r.items([[s('x'.repeat(2000))]], ['v'])
      r.truncationHint('full=true')
    })
    assert.match(result.content[0].text, /use full=true/)
    check('cell cut, unicode', (r) => {
      r.items([[s('日本語🙂'.repeat(400))]], ['v'])
      r.truncationHint('full=true')
    })
  })

  void it('a truncation hint michi rejects renders as an error body that still conforms', () => {
    const result = check('toon validation body', (r) => {
      r.items(rows(['a']), ['v'])
      r.truncationHint('h'.repeat(300))
    })
    assert.match(result.content[0].text, /toon_validation_failed/)
  })

  void it('KV responses', () => {
    check('kv', (r) => {
      r.kvItems([
        { key: 'name', value: s('michi') },
        { key: 'stars', value: i(12) },
        { key: 'ratio', value: f(0.25) },
        { key: 'open', value: b(true) },
        { key: 'owner', value: nul },
      ])
      r.hint('see the repo')
    })
  })

  void it('multi-section responses with every slot set', () => {
    const result = check('everything', (r) => {
      r.items(
        [
          [s('a'), i(1), f(1.5), b(true), nul],
          [s('b,c'), i(-2), f(0.1), b(false), nul],
        ],
        ['name', 'n', 'x', 'ok', 'gone'],
      )
      r.totalCount(400)
      r.hint('next: page=2')
      r.hint('filter with state=open')
      r.recoveryHint('list_pages', 'to see more')
      r.truncationHint('full=true')
      r.humanContent('Showing 2 of 400.')
      r.asError()
    })
    assert.equal(result.content.length, 2)
    assert.equal(result.isError, true)
  })

  void it('other renderers used as the human block', () => {
    const blocks = [
      renderStatus('tool', 'a tool', [{ key: 'db', value: s('up'), health: 'ok' }], ['all good']),
      renderPipeline([{ id: 'a', name: 'Build', status: 'completed' }]),
      renderPartialSuccess(['a'], [{ operation: 'b', reason: 'timeout' }], ['c']),
      renderRecovery([{ tool: 'retry', reason: 'transient' }]),
    ]
    for (const text of blocks) check('renderer output', (r) => r.humanContent(text))
  })

  void it('unicode and newline-heavy text', () => {
    const nasty = [
      'line1\nline2\r\nline3\rline4',
      '\n\n\n',
      'tab\there sep para',
      '日本語 עברית العربية 🙂 🏳️‍🌈 é ​‍',
      'astral: \u{1F600}\u{10FFFF}',
      'quotes " \' \\ / \b \f',
      '\u0001\u0002\u001f\u007f',
      '{"type":"text"}',
      '',
    ]
    for (const text of nasty) {
      check(`cell ${JSON.stringify(text)}`, (r) => r.items([[s(text)]], ['v']))
      check(`hint ${JSON.stringify(text)}`, (r) => {
        r.items(rows(['a']), ['v'])
        r.hint(text)
      })
      check(`human ${JSON.stringify(text)}`, (r) => r.humanContent(text))
      check(`field name ${JSON.stringify(text)}`, (r) => {
        r.items(rows(['a']), [text.slice(0, 20) || 'v'])
      })
    }
  })

  void it('two conversions of the same builder agree and do not consume it', () => {
    const r = new AgentResponse('thing')
    r.items(rows(['a']), ['v'])
    r.humanContent('h')
    const first = r.toCallToolResult()
    const second = r.toCallToolResult()
    assert.deepEqual(first, second)
    assertConforms(second, 'second conversion')
  })
})

/** Small deterministic PRNG (mulberry32), so a failure reproduces from the printed seed. */
const prng = (seed) => () => {
  seed = (seed + 0x6d2b79f5) | 0
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed)
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}

const PIECES = [
  ',', '"', '\\', '\n', '\r', '\t', ' ', ':', '[', ']', '{', '}', '-', '#', '|', ';', '.', '0', '1', '9', 'e', 'a', 'Z',
  'true', 'false', 'null', 'help[', 'error:', 'type', 'audience', '"type":"text"', 'é', '日', '🙂', ' ', '​',
  '\u0001', '\u007f', '\u0000', '/', "'",
]

const randomString = (rand) => {
  let out = ''
  for (let n = Math.floor(rand() * 12); n > 0; n--) out += PIECES[Math.floor(rand() * PIECES.length)]
  return out
}

/** The rejections michi documents: TOON and KV validation on the way in. Anything else is a bug. */
const DOCUMENTED = /^(toon_validation_failed|kv_validation_failed)\b/

void describe('fuzzed strings through toCallToolResult', () => {
  void it('every result conforms; the only throws are the documented validation errors', () => {
    const seed = 20261024
    const rand = prng(seed)
    let converted = 0
    let rejected = 0
    for (let n = 0; n < 2000; n++) {
      const cells = [randomString(rand), randomString(rand), randomString(rand)]
      const hints = [randomString(rand), randomString(rand)]
      const human = rand() < 0.5 ? randomString(rand) : undefined
      const label = `seed ${seed}, case ${n}: ${JSON.stringify({ cells, hints, human })}`
      let result
      try {
        const r = new AgentResponse(randomString(rand) || 't')
        r.items([cells.map(s)], [randomString(rand) || 'a', 'b', 'c'])
        for (const h of hints) r.hint(h)
        if (rand() < 0.3) r.recoveryHint(randomString(rand) || 'tool', rand() < 0.5 ? randomString(rand) : undefined)
        if (rand() < 0.3) r.truncationHint(randomString(rand))
        if (rand() < 0.3) r.asError()
        if (human !== undefined) r.humanContent(human)
        result = r.toCallToolResult()
      } catch (err) {
        assert.match(String(err?.message), DOCUMENTED, `undocumented error, ${label}`)
        rejected++
        continue
      }
      converted++
      assertConforms(result, label)
    }
    assert.ok(converted > 1000, `most generated inputs should convert (converted ${converted}, rejected ${rejected})`)
  })

  void it('kv responses with adversarial keys and values conform', () => {
    const rand = prng(777)
    let converted = 0
    for (let n = 0; n < 1000; n++) {
      const key = randomString(rand) || 'k'
      const value = randomString(rand)
      const label = `kv case ${n}: ${JSON.stringify({ key, value })}`
      let result
      try {
        const r = new AgentResponse('t')
        r.kvItems([{ key, value: s(value) }])
        r.hint(randomString(rand))
        result = r.toCallToolResult()
      } catch (err) {
        assert.match(String(err?.message), DOCUMENTED, `undocumented error, ${label}`)
        continue
      }
      converted++
      assertConforms(result, label)
    }
    assert.ok(converted > 0)
  })
})
