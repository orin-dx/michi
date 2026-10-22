import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
import {
  renderToon,
  emptyState,
  renderHints,
  appendHints,
  renderRecovery,
  truncate,
  AgentResponse,
  renderKv,
  renderAlreadyDone,
  parseRetryAfter,
  nextRetryDelay,
  isRetryableStatus,
  renderDomainError,
  renderStatus,
} from '../index.js'

void describe('renderToon', () => {
  void it('renders a basic list', () => {
    const out = renderToon({
      typeName: 'issue',
      fields: ['number', 'title', 'state'],
      rows: [
        [
          { type: 'int', intVal: 42 },
          { type: 'str', strVal: 'Fix login' },
          { type: 'str', strVal: 'open' },
        ],
      ],
      totalCount: 100,
      hints: ['Call get_issue with number=<number>'],
    })
    assert.ok(out.startsWith('issue[1]{number,title,state}:'))
    assert.ok(out.includes('42,Fix login,open'))
    assert.ok(out.includes('totalCount: 100'))
    assert.ok(out.includes('help[1]:'))
  })

  void it('renders null values', () => {
    const out = renderToon({
      typeName: 'item',
      fields: ['a', 'b'],
      rows: [[{ type: 'str', strVal: 'x' }, { type: 'null' }]],
      hints: [],
    })
    assert.ok(out.includes('x,'))
  })

  void it('throws a catchable JS error instead of hanging on an oversized rows array', () => {
    const rows = Array.from({ length: 100_001 }, () => [{ type: 'null' }])
    assert.throws(() => renderToon({ typeName: 'item', fields: ['a'], rows, hints: [] }), /rows length/)
  })

  void it('throws when a declared type is missing its payload field, not silently defaulting', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'int' }]], hints: [] }),
      /intVal/,
    )
  })

  void it('throws on an unrecognized type string, even with a valid payload present', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'integer', intVal: 42 }]], hints: [] }),
      /integer/,
    )
  })
})

void describe('emptyState', () => {
  void it('returns empty block', () => {
    const out = emptyState('issue')
    assert.strictEqual(out, 'issue: []\ntotalCount: 0')
  })
})

void describe('renderHints', () => {
  void it('renders hint block', () => {
    const out = renderHints(['hint one', 'hint two'])
    // TOON 4.4 inline primitive array; hints are quoted only when §7.2 requires it.
    assert.strictEqual(out, 'help[2]: hint one,hint two')
    assert.strictEqual(renderHints(['a, b']), 'help[1]: "a, b"')
  })

  void it('returns empty for no hints', () => {
    assert.strictEqual(renderHints([]), '')
  })
})

void describe('appendHints', () => {
  void it('appends a help block to an existing body', () => {
    assert.strictEqual(appendHints('body\n', ['do this']), 'body\nhelp[1]: do this')
    assert.strictEqual(appendHints('t: []', ['do this']), 't: []\nhelp[1]: do this')
  })
})

void describe('renderRecovery', () => {
  void it('renders a recovery block', () => {
    // TOON 4.4 tabular array; params is the quoted compact JSON object, an absent reason is null.
    const out = renderRecovery([{ tool: 'retry', reason: 'rate limited' }, { tool: 'reload' }])
    assert.strictEqual(out, 'recovery[2]{tool,params,reason}:\n  retry,"{}",rate limited\n  reload,"{}",null')
  })
})

void describe('truncate', () => {
  void it('returns short content unchanged', () => {
    assert.strictEqual(truncate('hello', 100, 'full=true'), 'hello')
  })

  void it('truncates long content and reports the chars cut', () => {
    // " (183 more chars — use full=true)" is 33 chars: 17 kept, and 17 + 183 = 200.
    const out = truncate('a'.repeat(200), 50, 'full=true')
    assert.strictEqual(out, `${'a'.repeat(17)} (183 more chars — use full=true)`)
  })

  void it('omits the use clause for an empty hint', () => {
    assert.strictEqual(truncate('a'.repeat(100), 40, ''), `${'a'.repeat(24)} (76 more chars)`)
  })

  void it('falls back to an ellipsis when the cap is too small for the suffix', () => {
    assert.strictEqual(truncate('a'.repeat(100), 3, 'x'), 'aa…')
    assert.strictEqual(truncate('a'.repeat(100), 1, 'x'), '…')
    assert.strictEqual(truncate('a'.repeat(100), 0, 'x'), '')
  })
})

void describe('truncation hint', () => {
  const longRow = [[{ type: 'str', strVal: 'a'.repeat(500) }]]

  void it('renderToon names no parameter without truncationHint', () => {
    const out = renderToon({ typeName: 't', fields: ['a'], rows: longRow, hints: [] })
    assert.strictEqual(out, `t[1]{a}:\n  ${'a'.repeat(183)} (317 more chars)`)
  })

  void it('renderToon appends the caller truncationHint', () => {
    const out = renderToon({ typeName: 't', fields: ['a'], rows: longRow, hints: [], truncationHint: 'full=true' })
    assert.strictEqual(out, `t[1]{a}:\n  ${'a'.repeat(167)} (333 more chars — use full=true)`)
  })

  void it('AgentResponse.truncationHint passes through to the TOON body', () => {
    const r = new AgentResponse('t')
    r.truncationHint('full=true')
    r.items(longRow, ['a'])
    assert.strictEqual(r.renderToon(), `t[1]{a}:\n  ${'a'.repeat(167)} (333 more chars — use full=true)`)
  })
})

void describe('standalone helpers', () => {
  void it('renderKv formats key-value block', () => {
    const out = renderKv(
      [{ key: 'name', value: { type: 'str', strVal: 'michi' } }],
      1,
      ['hint 1']
    )
    assert.strictEqual(out, 'name: michi\ntotalCount: 1\nhelp[1]: hint 1')
  })

  void it('renderKv writes TOON primitives: null, canonical rounded floats, quoted strings', () => {
    const out = renderKv(
      [
        { key: 'gone', value: { type: 'null' } },
        { key: 'score', value: { type: 'float', floatVal: 1.5, decimalsVal: 3 } },
        { key: 'note', value: { type: 'str', strVal: 'a: b' } },
      ],
      null,
      []
    )
    assert.strictEqual(out, 'gone: null\nscore: 1.5\nnote: "a: b"')
  })

  void it('renderKv rejects a duplicate or reserved key', () => {
    const k = (key) => ({ key, value: { type: 'null' } })
    assert.throws(
      () => renderKv([k('a'), k('a')], null, []),
      (err) => err.message === 'kv_validation_failed: key "a" appears more than once'
    )
    assert.throws(() => renderKv([k('help')], null, []), /kv_validation_failed: key "help" is a reserved document key/)
  })

  void it('renderAlreadyDone formats no-op block', () => {
    const out = renderAlreadyDone('create_issue', 'Already exists', ['view issue 42'])
    assert.strictEqual(out, 'operation: create_issue\nstatus: already_done\nsummary: Already exists\nhelp[1]: view issue 42')
  })

  void it('parseRetryAfter parses integer seconds', () => {
    assert.strictEqual(parseRetryAfter('120'), 120)
  })

  void it('nextRetryDelay calculates exponential backoff', () => {
    const delay = nextRetryDelay(3, 100, 1000, 0.0, 0.5, 0)
    assert.strictEqual(typeof delay, 'number')
    assert.ok(delay >= 100)
  })

  void it('nextRetryDelay throws on NaN delay input', () => {
    assert.throws(
      () => nextRetryDelay(3, NaN, 1000, 0.0, 0.5, 0),
      (err) => err.message === 'expected a finite number, got NaN',
    )
  })

  void it('nextRetryDelay throws on negative delay input', () => {
    assert.throws(() => nextRetryDelay(3, -1, 1000, 0.0, 0.5, 0), /finite non-negative/)
  })

  void it('nextRetryDelay throws on Infinity delay input', () => {
    assert.throws(
      () => nextRetryDelay(3, Infinity, 1000, 0.0, 0.5, 0),
      (err) => err.message === 'expected a finite number, got inf',
    )
  })

  void it('isRetryableStatus identifies 429 and 503 as retryable', () => {
    assert.strictEqual(isRetryableStatus(429), true)
    assert.strictEqual(isRetryableStatus(503), true)
    assert.strictEqual(isRetryableStatus(404), false)
  })

  void it('rejects status = 70000 (AC-015)', () => {
    assert.throws(
      () => isRetryableStatus(70000),
      (err) => err.message === 'expected an integer in [100, 599], got 70000'
    )
  })

  void it('renderDomainError formats error card and github annotation', () => {
    const card = renderDomainError('not_found', 'Item not found: 42', ['check ID'])
    assert.strictEqual(card, 'error: not_found\nmessage: "Item not found: 42"\nexit_code: 1\nhelp[1]: check ID')

    const gh = renderDomainError('not_found', 'Item not found', [], true)
    assert.strictEqual(gh, '::error title=not_found::Item not found')
  })

  void it('renderDomainError throws on unknown error code', () => {
    assert.throws(() => renderDomainError('made_up_code', 'msg', []), /unknown error code/)
  })

  void it('renderStatus formats orientation block', () => {
    const out = renderStatus('my_tool', 'Does work', [
      { key: 'cache', value: { type: 'str', strVal: 'ready' }, health: 'ok' },
      { key: 'db', value: { type: 'int', intVal: 3 }, health: 'degraded: slow' },
    ])
    assert.strictEqual(
      out,
      'tool: my_tool\ndescription: Does work\nitems[2]{key,value,health,reason}:\n  cache,ready,ok,null\n  db,3,degraded,slow'
    )
  })
})

void describe('AgentResponse', () => {
  void it('builds a TOON response with hints via chained calls', () => {
    const r = new AgentResponse('issues')
    r.items([[{ type: 'int', intVal: 1 }]], ['id'])
    r.hint('do this')
    const out = r.renderToon()
    assert.ok(out.startsWith('issues[1]{id}:'))
    assert.ok(out.includes('help[1]:'))
  })

  void it('builds a KV response', () => {
    const r = new AgentResponse('issue')
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 42 } }])
    assert.ok(r.renderKv().includes('id:'))
  })

  void it('renderJson reflects asError', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    r.asError()
    assert.ok(r.renderJson().includes('"isError":true'))
  })

  void it('mutators keep working after render calls (render takes &self, not &mut self)', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    r.renderKv()
    r.hint('still works')
    assert.ok(r.renderHintsOnly().includes('still works'))
  })

  void it('renderToon/renderKv are slot-specific, not last-call-wins', () => {
    const r = new AgentResponse('issues')
    r.items([[{ type: 'int', intVal: 1 }]], ['id'])
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 99 } }])
    const toon = r.renderToon()
    const kv = r.renderKv()
    assert.ok(toon.startsWith('issues[1]{id}:'), `got: ${toon}`)
    assert.ok(kv.includes('id: 99'), `got: ${kv}`)
    assert.notStrictEqual(toon, kv)
  })
})

void describe('toCallToolResult', () => {
  void it('returns MCP-conformant content blocks with type/annotations.audience', () => {
    const r = new AgentResponse('issue')
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 1 } }])
    const result = r.toCallToolResult()
    assert.strictEqual(result.content.length, 1)
    assert.strictEqual(result.content[0].type, 'text')
    assert.deepStrictEqual(result.content[0].annotations.audience, ['assistant'])
    assert.strictEqual(result.isError, false)
    assert.strictEqual(typeof result.structuredContent, 'object')
    assert.strictEqual(result.structuredContent.isError, false)
  })

  void it('reflects isError and includes a user-audience block from humanContent', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    r.asError()
    r.humanContent('friendly summary')
    const result = r.toCallToolResult()
    assert.strictEqual(result.isError, true)
    assert.strictEqual(result.structuredContent.isError, true)
    assert.strictEqual(result.content.length, 2)
    assert.strictEqual(result.content[1].type, 'text')
    assert.deepStrictEqual(result.content[1].annotations.audience, ['user'])
  })

  void it('marks invalid TOON rows as an error on every surface', () => {
    const r = new AgentResponse('issues')
    r.items([[{ type: 'int', intVal: 1 }]], ['id', 'state'])
    const result = r.toCallToolResult()
    assert.ok(result.content[0].text.startsWith('error: toon_validation_failed\n'), `got: ${result.content[0].text}`)
    assert.strictEqual(result.isError, true)
    assert.strictEqual(result.structuredContent.isError, true)
    assert.ok(r.renderJson().includes('"isError":true'))
  })
})

void describe('renderFor / hasHumanContent', () => {
  void it('assistant matches the agent rendering', () => {
    const r = new AgentResponse('issue')
    r.kvItems([{ key: 'id', value: { type: 'int', intVal: 1 } }])
    assert.strictEqual(r.renderFor('assistant'), r.renderKv())
  })

  void it('user returns humanContent when set', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    r.humanContent('hi there')
    assert.strictEqual(r.renderFor('user'), 'hi there')
    assert.strictEqual(r.hasHumanContent(), true)
  })

  void it('user falls back to agent rendering when humanContent was never set', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    assert.strictEqual(r.hasHumanContent(), false)
    assert.strictEqual(r.renderFor('user'), r.renderKv())
  })

  void it('rejects an unknown audience', () => {
    const r = new AgentResponse('t')
    r.kvItems([])
    assert.throws(() => r.renderFor('nonsense'), /nonsense/)
  })
})

void describe('int64 boundary (SPEC-NAPI-POINTFIX-001)', () => {
  void it('AC-001 renders a positive int beyond i32::MAX losslessly', () => {
    const out = renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'int', intVal: 1755000000000 }]], hints: [] })
    assert.strictEqual(out, 't[1]{a}:\n  1755000000000')
  })
  void it('AC-002 renders a negative int beyond i32::MIN losslessly', () => {
    const out = renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'int', intVal: -1755000000000 }]], hints: [] })
    assert.strictEqual(out, 't[1]{a}:\n  -1755000000000')
  })
  void it('AC-003 renderKv renders an int beyond i32::MAX losslessly', () => {
    const out = renderKv([{ key: 'id', value: { type: 'int', intVal: 1755000000000 } }], null, [])
    assert.strictEqual(out, 'id: 1755000000000')
  })
  void it('renderToon rejects a fractional intVal', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'int', intVal: 1.5 }]], hints: [] }),
      (err) => err.message.includes('expected an integer in [-9007199254740991, 9007199254740991], got 1.5')
    )
  })
})

void describe('render_toon validate() surfacing (SPEC-NAPI-POINTFIX-001)', () => {
  void it('AC-005 rejects a row with fewer values than declared fields', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a', 'b'], rows: [[{ type: 'str', strVal: 'x' }]], hints: [] }),
      (err) => err.message === 'toon_validation_failed: row 0 has 1 values but 2 fields declared'
    )
  })
  void it('AC-006 rejects a row with more values than declared fields', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'str', strVal: 'x' }, { type: 'str', strVal: 'y' }]], hints: [] }),
      (err) => err.message === 'toon_validation_failed: row 0 has 2 values but 1 fields declared'
    )
  })
  void it('AC-009 quotes a type_name containing a structural character (TOON 4.4 §7.3)', () => {
    const out = renderToon({ typeName: 'a[b]', fields: ['x'], rows: [[{ type: 'str', strVal: 'v' }]], hints: [] })
    assert.strictEqual(out, '"a[b]"[1]{x}:\n  v')
  })
  void it('AC-009b rejects a reserved type_name', () => {
    assert.throws(
      () => renderToon({ typeName: 'help', fields: ['x'], rows: [[{ type: 'str', strVal: 'v' }]], hints: [] }),
      (err) => err.message === 'toon_validation_failed: type_name "help" is a reserved document key (totalCount, help, recovery)'
    )
  })
  void it('AC-010 quotes a field name containing a structural character (TOON 4.4 §7.3)', () => {
    const out = renderToon({ typeName: 't', fields: ['a,b'], rows: [[{ type: 'str', strVal: 'v' }]], hints: [] })
    assert.strictEqual(out, 't[1]{"a,b"}:\n  v')
  })
  void it('AC-010c rejects a repeated field name', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a', 'a'], rows: [], hints: [] }),
      (err) => err.message === 'toon_validation_failed: field "a" appears more than once'
    )
  })
  void it('AC-010b renderToon throws with a toon_validation_failed-prefixed message for row-arity errors', () => {
    assert.throws(
      () =>
        renderToon({
          typeName: 'issue',
          fields: ['a', 'b'],
          rows: [[{ type: 'str', strVal: 'only-one-cell' }]],
          hints: [],
        }),
      /toon_validation_failed/
    )
  })
})

void describe('numeric boundary agreement (SPEC-ARCH-003)', () => {
  void it('totalCount rejects out-of-domain input identically across renderToon, renderKv, and AgentResponse', () => {
    for (const bad of [-1, 1.5]) {
      const substring = `got ${bad}`
      assert.throws(
        () => renderToon({ typeName: 't', fields: ['a'], rows: [], totalCount: bad, hints: [] }),
        (err) => err.message.includes('expected a non-negative integer no greater than 9007199254740991') && err.message.includes(substring)
      )
      assert.throws(
        () => renderKv([], bad, []),
        (err) => err.message.includes('expected a non-negative integer no greater than 9007199254740991') && err.message.includes(substring)
      )
      assert.throws(
        () => new AgentResponse('t').totalCount(bad),
        (err) => err.message.includes('expected a non-negative integer no greater than 9007199254740991') && err.message.includes(substring)
      )
    }
  })

  void it('totalCount entry points agree on in-domain values including 2147483648', () => {
    for (const good of [0, 1, 100, 2147483648]) {
      const toonOut = renderToon({ typeName: 't', fields: ['a'], rows: [], totalCount: good, hints: [] })
      assert.ok(toonOut.includes(`totalCount: ${good}`), `renderToon disagreed for ${good}: ${toonOut}`)

      const kvOut = renderKv([{ key: 'id', value: { type: 'int', intVal: 1 } }], good, [])
      assert.ok(kvOut.includes(`totalCount: ${good}`), `renderKv disagreed for ${good}: ${kvOut}`)

      const r = new AgentResponse('t')
      r.items([[{ type: 'int', intVal: 1 }]], ['a'])
      r.totalCount(good)
      const agentOut = r.renderToon()
      assert.ok(agentOut.includes(`totalCount: ${good}`), `AgentResponse.totalCount disagreed for ${good}: ${agentOut}`)
    }
  })
})

void describe('rejection instead of coercion (SPEC-ARCH-003 AC-009)', () => {
  void it('truncate rejects a negative maxChars', () => {
    assert.throws(
      () => truncate('hello', -5, 'full=true'),
      (err) => err.message.includes('expected a non-negative integer no greater than 9007199254740991, got -5')
    )
  })

  void it('renderKv rejects a decimalsVal above 20', () => {
    assert.throws(
      () => renderKv([{ key: 'score', value: { type: 'float', floatVal: 1.0, decimalsVal: 21 } }], null, []),
      (err) => err.message.includes('expected an integer in [0, 20], got 21')
    )
  })

  void it('renderToon rejects a NaN floatVal', () => {
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'float', floatVal: NaN }]], hints: [] }),
      (err) => err.message.includes('expected a finite number, got NaN')
    )
  })

  void it('renderKv defaults absent decimalsVal to 6', () => {
    // Rounded to 6 places, then written canonically (TOON 4.4 §2): decimals bound precision, never pad.
    const out = renderKv([{ key: 'score', value: { type: 'float', floatVal: 1.23456789 } }], null, [])
    assert.strictEqual(out, 'score: 1.234568')
  })
})

void describe('index.d.ts keeps plain number types (SPEC-ARCH-003 AC-012)', () => {
  void it('declares plain number types for all converted positions and leaks no newtype names', () => {
    const dts = fs.readFileSync(path.join(__dirname, '..', 'index.d.ts'), 'utf8')
    for (const expected of [
      'intVal?: number',
      'floatVal?: number',
      'decimalsVal?: number',
      'totalCount?: number',
      'totalCount: number | undefined | null',
      'maxChars: number',
      'totalCount(n: number): void',
    ]) {
      assert.ok(dts.includes(expected), `index.d.ts missing declaration: ${expected}`)
    }
    for (const forbidden of ['bigint', 'BigInt', 'JsCount', 'JsInt', 'JsRanged', 'JsDecimals', 'JsFloat']) {
      assert.ok(!dts.includes(forbidden), `index.d.ts leaked forbidden string: ${forbidden}`)
    }
  })
})

void describe('jitter_seed / jitter_factor rejection (SPEC-ARCH-004)', () => {
  void it('rejects jitter_seed = 1.5 with the exact range-check message (AC-001)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 0.2, 1.5, 0),
      (err) => err.message === 'expected a finite number in [0.0, 1.0], got 1.5'
    )
  })

  void it('rejects jitter_seed = -0.1 with the exact range-check message (AC-002)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 0.2, -0.1, 0),
      (err) => err.message === 'expected a finite number in [0.0, 1.0], got -0.1'
    )
  })

  void it('rejects jitter_factor = NaN with the delegated finiteness message (AC-003)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, NaN, 0.5, 0),
      (err) => err.message === 'expected a finite number, got NaN'
    )
  })

  void it('rejects jitter_seed = NaN with the delegated finiteness message (AC-031)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 0.2, NaN, 0),
      (err) => err.message === 'expected a finite number, got NaN'
    )
  })

  void it('rejects jitter_factor = 5.0 instead of clamping to 1.0 (AC-004b)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 5.0, 0.5, 0),
      (err) => err.message === 'expected a finite number in [0.0, 1.0], got 5'
    )
  })

  void it('accepts jitter_seed / jitter_factor at 0.0, 0.5, and 1.0 with exact delay values (AC-004)', () => {
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.0, 0.5, 0), 100)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.5, 0.5, 0), 125)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 1.0, 0.5, 0), 150)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.0, 0), 100)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0), 110)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 1.0, 0), 120)
  })
})

void describe('base_delay_ms / max_delay_ms boundary (SPEC-ARCH-004)', () => {
  void it('rejects base_delay_ms = -1 (AC-005)', () => {
    assert.throws(
      () => nextRetryDelay(3, -1, 1000, 0.2, 0.5, 0),
      (err) => err.message === 'expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got -1'
    )
  })

  void it('rejects max_delay_ms = NaN with the delegated finiteness message (AC-006)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, NaN, 0.2, 0.5, 0),
      (err) => err.message === 'expected a finite number, got NaN'
    )
  })

  void it('rejects max_delay_ms = Infinity with the delegated finiteness message (AC-006b)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, Infinity, 0.2, 0.5, 0),
      (err) => err.message === 'expected a finite number, got inf'
    )
  })

  void it('rejects base_delay_ms and max_delay_ms = 2e22 as Duration-overflowing (AC-007)', () => {
    const expected = (err) =>
      err.message ===
      'expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got 20000000000000000000000'
    assert.throws(() => nextRetryDelay(3, 2e22, 1000, 0.2, 0.5, 0), expected)
    assert.throws(() => nextRetryDelay(3, 100, 2e22, 0.2, 0.5, 0), expected)
  })

  void it('rejects base_delay_ms = 1.8446744073709552e22, exactly 2^64 seconds-equivalent (AC-007b)', () => {
    assert.throws(
      () => nextRetryDelay(3, 1.8446744073709552e22, 1000, 0.2, 0.5, 0),
      (err) =>
        err.message ===
        'expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got 18446744073709552000000'
    )
  })

  void it('accepts base_delay_ms and max_delay_ms at 1.844674407370955e22, one f64 ULP below the overflow threshold (AC-030)', () => {
    assert.strictEqual(typeof nextRetryDelay(3, 1.844674407370955e22, 1000, 0.2, 0.5, 0), 'number')
    assert.strictEqual(typeof nextRetryDelay(3, 100, 1.844674407370955e22, 0.2, 0.5, 0), 'number')
  })
})

void describe('retry_after_ms boundary (SPEC-ARCH-004)', () => {
  void it('rejects retry_after_ms = -1 (AC-008)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0, -1),
      (err) => err.message === 'expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got -1'
    )
  })

  void it('rejects retry_after_ms = 2e22 as Duration-overflowing (AC-009)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0, 2e22),
      (err) =>
        err.message ===
        'expected a finite non-negative number convertible to a Duration (v / 1000.0 < u64::MAX), got 20000000000000000000000'
    )
  })

  void it('succeeds with delay 110 when retry_after_ms is omitted, undefined, or null (AC-010, AC-025)', () => {
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0), 110)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0, undefined), 110)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0, null), 110)
  })

  void it('succeeds with delay 500 when retry_after_ms = 500 is present and in-domain (AC-026)', () => {
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0, 500), 500)
  })

  void it('succeeds with delay 110 when retry_after_ms = 50 is present but loses to the jittered delay', () => {
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0, 50), 110)
  })
})

void describe('max_retries / attempt boundary (SPEC-ARCH-004)', () => {
  void it('rejects max_retries = -1 (AC-011)', () => {
    assert.throws(
      () => nextRetryDelay(-1, 100, 1000, 0.2, 0.5, 0),
      (err) => err.message === 'expected an integer in [0, 4294967295], got -1'
    )
  })

  void it('rejects attempt = -1 (AC-012)', () => {
    assert.throws(
      () => nextRetryDelay(3, 100, 1000, 0.2, 0.5, -1),
      (err) => err.message === 'expected an integer in [0, 4294967295], got -1'
    )
  })

  void it('rejects max_retries or attempt = 4294967296 (AC-013)', () => {
    const expected = (err) => err.message === 'expected an integer in [0, 4294967295], got 4294967296'
    assert.throws(() => nextRetryDelay(4294967296, 100, 1000, 0.2, 0.5, 0), expected)
    assert.throws(() => nextRetryDelay(3, 100, 1000, 0.2, 0.5, 4294967296), expected)
  })

  void it('rejects max_retries or attempt = 2.5 (AC-014)', () => {
    const expected = (err) => err.message === 'expected an integer in [0, 4294967295], got 2.5'
    assert.throws(() => nextRetryDelay(2.5, 100, 1000, 0.2, 0.5, 0), expected)
    assert.throws(() => nextRetryDelay(3, 100, 1000, 0.2, 0.5, 2.5), expected)
  })

  void it('succeeds with delay 220 for attempt = 1, max_retries = 3 (AC-027)', () => {
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 1), 220)
  })

  void it('returns null for max_retries = 0; delay 110 for attempt = 0, max_retries = 3 (AC-028)', () => {
    assert.strictEqual(nextRetryDelay(0, 100, 1000, 0.2, 0.5, 0), null)
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.2, 0.5, 0), 110)
  })

  void it('accepts max_retries = 4294967295 (delay 110); returns null when attempt also = 4294967295 (AC-029)', () => {
    assert.strictEqual(nextRetryDelay(4294967295, 100, 1000, 0.2, 0.5, 0), 110)
    assert.strictEqual(nextRetryDelay(4294967295, 100, 1000, 0.2, 0.5, 4294967295), null)
  })
})

void describe('isRetryableStatus boundary (SPEC-ARCH-004)', () => {
  void it('rejects status = 50 (AC-016)', () => {
    assert.throws(
      () => isRetryableStatus(50),
      (err) => err.message === 'expected an integer in [100, 599], got 50'
    )
  })

  void it('classifies 429/502/503/504 as retryable and 100/200/404/599 as not (AC-017)', () => {
    for (const retryable of [429, 502, 503, 504]) {
      assert.strictEqual(isRetryableStatus(retryable), true, `expected ${retryable} to be retryable`)
    }
    for (const notRetryable of [100, 200, 404, 599]) {
      assert.strictEqual(isRetryableStatus(notRetryable), false, `expected ${notRetryable} to not be retryable`)
    }
  })

  void it('rejects a fractional status (AC-018)', () => {
    assert.throws(
      () => isRetryableStatus(429.5),
      (err) => err.message === 'expected an integer in [100, 599], got 429.5'
    )
  })
})

void describe('index.d.ts keeps plain number types for the resilience domain (SPEC-ARCH-004 AC-022)', () => {
  void it('declares plain number types for nextRetryDelay and isRetryableStatus, and leaks no newtype names', () => {
    const dts = fs.readFileSync(path.join(__dirname, '..', 'index.d.ts'), 'utf8')
    assert.ok(
      dts.includes(
        'export declare function nextRetryDelay(maxRetries: number, baseDelayMs: number, maxDelayMs: number, jitterFactor: number, jitterSeed: number, attempt: number, retryAfterMs?: number | undefined | null): number | null'
      ),
      `index.d.ts missing exact nextRetryDelay declaration, got: ${dts.split('\n').filter((l) => l.includes('nextRetryDelay')).join('\n')}`
    )
    assert.ok(
      dts.includes('export declare function isRetryableStatus(status: number): boolean'),
      `index.d.ts missing exact isRetryableStatus declaration, got: ${dts.split('\n').filter((l) => l.includes('isRetryableStatus')).join('\n')}`
    )
    for (const forbidden of ['bigint', 'BigInt', 'JsRetryCount', 'JsDelayMillis', 'JsUnitInterval', 'JsHttpStatus', 'JsRanged']) {
      assert.ok(!dts.includes(forbidden), `index.d.ts leaked forbidden string: ${forbidden}`)
    }
  })
})

void describe('input limits bound the work (capacity limits)', () => {
  // Loose bound: the old eager conversion took 3.4 s and 2.6 GiB to reject a 100,001-row input.
  const FAST_MS = 1000
  const timed = (fn) => {
    const start = performance.now()
    let error
    try {
      fn()
    } catch (e) {
      error = e
    }
    return { error, ms: performance.now() - start }
  }
  const nullCell = { type: 'null' }
  const wideRows = (n, width) => Array.from({ length: n }, () => Array.from({ length: width }, () => nullCell))

  void it('rejects a 100,001 x 100 renderToon input before converting it', () => {
    const rows = wideRows(100_001, 100)
    const { error, ms } = timed(() => renderToon({ typeName: 't', fields: ['a'], rows, hints: [] }))
    assert.ok(error, 'expected a rejection')
    assert.strictEqual(error.message, 'rows length 100001 exceeds maximum of 100000')
    assert.ok(ms < FAST_MS, `rejection took ${ms} ms`)
  })

  void it('rejects 100,000 x 100 cells by the per-call cell cap before reading any cell', () => {
    const rows = wideRows(100_000, 100)
    const fields = Array.from({ length: 100 }, (_, i) => `f${i}`)
    const viaRender = timed(() => renderToon({ typeName: 't', fields, rows, hints: [] }))
    assert.match(viaRender.error?.message ?? '', /^cell count \d+ exceeds maximum of 1000000$/)
    assert.ok(viaRender.ms < FAST_MS, `renderToon rejection took ${viaRender.ms} ms`)

    const r = new AgentResponse('t')
    const viaItems = timed(() => r.items(rows, fields))
    assert.match(viaItems.error?.message ?? '', /^cell count \d+ exceeds maximum of 1000000$/)
    assert.ok(viaItems.ms < FAST_MS, `AgentResponse.items rejection took ${viaItems.ms} ms`)
  })

  void it('charges a JS string shared by many cells once per cell against the 64 MiB byte budget', () => {
    // 10k x 10 cells referencing one 100 KB string would copy out ~10 GB.
    const shared = { type: 'str', strVal: 'x'.repeat(100_000) }
    const rows = Array.from({ length: 10_000 }, () => Array.from({ length: 10 }, () => shared))
    const fields = Array.from({ length: 10 }, (_, i) => `f${i}`)
    const { error, ms } = timed(() => renderToon({ typeName: 't', fields, rows, hints: [] }))
    assert.match(error?.message ?? '', /^input byte total \d+ exceeds maximum of 67108864$/)
    assert.ok(ms < FAST_MS, `rejection took ${ms} ms`)

    const r = new AgentResponse('t')
    r.items([[{ type: 'int', intVal: 1 }]], ['id'])
    assert.throws(() => r.items(rows, fields), /^Error: input byte total \d+ exceeds maximum of 67108864$/)
    assert.strictEqual(r.renderToon(), 't[1]{id}:\n  1', 'a rejected items() call leaves the builder unchanged')

    const hint = 'h'.repeat(1_000_000)
    assert.throws(
      () => renderHints(Array.from({ length: 100 }, () => hint)),
      /input byte total \d+ exceeds maximum of 67108864/
    )
  })

  void it('caps each cell and hint at 1 MiB, counted in UTF-8 bytes', () => {
    const big = 'x'.repeat(1_048_577)
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'str', strVal: big }]], hints: [] }),
      (err) => err.message === 'strVal byte length (at least 1048577) exceeds maximum of 1048576'
    )
    // 600,000 two-byte chars: under the cap in UTF-16 units, over it in UTF-8 bytes.
    const wide = 'é'.repeat(600_000)
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'str', strVal: wide }]], hints: [] }),
      (err) => err.message === 'strVal byte length 1200000 exceeds maximum of 1048576'
    )
    assert.throws(() => renderHints([big]), /hint byte length .* exceeds maximum of 1048576/)
    assert.throws(() => renderKv([{ key: 'k', value: { type: 'str', strVal: big } }], null, []), /strVal byte length/)
    assert.throws(() => new AgentResponse('t').hint(big), /hint byte length 1048577 exceeds maximum of 1048576/)
    assert.throws(() => renderRecovery([{ tool: 'retry', reason: big }]), /reason byte length/)
    assert.throws(() => renderDomainError('not_found', big, []), /message byte length/)
    // Exactly 1 MiB is accepted.
    const atCap = 'x'.repeat(1_048_576)
    const out = renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'str', strVal: atCap }]], hints: [] })
    assert.ok(out.startsWith('t[1]{a}:\n  xxx'))
  })

  void it('caps type names, field names, keys and truncation hints at 4 KiB', () => {
    const long = 'n'.repeat(4097)
    const opts = (over) => ({ typeName: 't', fields: ['a'], rows: [], hints: [], ...over })
    assert.throws(() => renderToon(opts({ typeName: long })), /^Error: typeName byte length .* exceeds maximum of 4096$/)
    assert.throws(() => renderToon(opts({ fields: [long] })), /^Error: field name byte length .* exceeds maximum of 4096$/)
    assert.throws(
      () => renderToon(opts({ truncationHint: long })),
      /^Error: truncationHint byte length .* exceeds maximum of 4096$/
    )
    assert.throws(() => renderKv([{ key: long, value: { type: 'null' } }], null, []), /key byte length/)
    assert.throws(() => new AgentResponse('t').items([], [long]), /field name byte length/)
    assert.throws(() => new AgentResponse('t').truncationHint(long), /truncationHint byte length 4097 exceeds maximum of 4096/)
    assert.throws(() => new AgentResponse('t').recoveryHint(long), /tool byte length 4097 exceeds maximum of 4096/)
    assert.strictEqual(renderToon(opts({ typeName: 'n'.repeat(4096) })), `${'n'.repeat(4096)}: []`)
  })

  void it('surfaces michi-core shape caps as toon_validation_failed', () => {
    const hint = 'h'.repeat(257)
    assert.throws(
      () => renderToon({ typeName: 't', fields: ['a'], rows: [], hints: [], truncationHint: hint }),
      (err) => err.message === 'toon_validation_failed: truncation_hint length 257 chars exceeds maximum of 256'
    )
    const r = new AgentResponse('t')
    r.items([[{ type: 'str', strVal: 'x' }]], ['a'])
    r.truncationHint(hint)
    const result = r.toCallToolResult()
    assert.ok(result.content[0].text.startsWith('error: toon_validation_failed\n'), result.content[0].text)
    assert.strictEqual(result.isError, true)
    assert.strictEqual(result.structuredContent.isError, true)
  })

  void it('rejects non-array collection arguments with a plain message', () => {
    assert.throws(() => renderHints('nope'), /^Error: hints must be an array$/)
    assert.throws(() => renderToon({ typeName: 't', fields: 'a', rows: [], hints: [] }), /fields must be an array/)
    assert.throws(() => renderToon({ typeName: 't', fields: ['a'], rows: [5], hints: [] }), /each row must be an array/)
    assert.throws(() => renderToon({ typeName: 't', fields: ['a'], rows: [[5]], hints: [] }), /JsToonValue must be an object/)
    assert.throws(() => renderToon({ fields: [], rows: [], hints: [] }), /Missing field `typeName`/)
  })

  void it('keeps optional arguments optional', () => {
    const item = { key: 'db', value: { type: 'int', intVal: 3 } }
    assert.strictEqual(renderStatus('t', 'd', [item], null), renderStatus('t', 'd', [item]))
    assert.strictEqual(renderStatus('t', 'd', [item], undefined), renderStatus('t', 'd', [item]))
    assert.strictEqual(renderDomainError('not_found', 'm', [], undefined), renderDomainError('not_found', 'm', []))
    const out = renderToon({
      typeName: 't',
      fields: ['a'],
      rows: [[{ type: 'str', strVal: 'x', intVal: undefined }]],
      hints: [],
      totalCount: undefined,
      truncationHint: undefined,
    })
    assert.strictEqual(out, 't[1]{a}:\n  x')
    // As before, an object field set to null is a wrong-typed value, not an absent one.
    assert.throws(() => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'int', intVal: null }]], hints: [] }))
    assert.throws(() => renderToon({ typeName: 't', fields: ['a'], rows: [[{ type: 'str', strVal: null }]], hints: [] }))
  })
})

void describe('full in-domain regression (SPEC-ARCH-004 AC-021)', () => {
  void it('nextRetryDelay and isRetryableStatus in-domain results are unchanged from before this spec', () => {
    assert.strictEqual(nextRetryDelay(3, 100, 1000, 0.0, 0.5, 0), 100)
    assert.strictEqual(isRetryableStatus(429), true)
    assert.strictEqual(isRetryableStatus(503), true)
    assert.strictEqual(isRetryableStatus(404), false)
  })
})
