// Interoperability: michi's TOON output, decoded by the reference implementation (@toon-format/toon, pinned to the
// TOON version michi targets). Expected values are written by hand, never produced by michi, so a disagreement
// between the two implementations fails here. The decoder is a dev dependency of this package only; no Rust crate
// depends on it. See docs/research/toon-interoperability.md.
import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { decode } from '@toon-format/toon'
import { renderToon, renderHints } from '../index.js'

const s = (strVal) => ({ type: 'str', strVal })
const i = (intVal) => ({ type: 'int', intVal })
const f = (floatVal) => ({ type: 'float', floatVal })
const b = (boolVal) => ({ type: 'bool', boolVal })
const nul = { type: 'null' }

/** Renders one table and returns what the reference decoder reads back. */
const roundTrip = (opts) => decode(renderToon({ hints: [], ...opts }))

/** One-column table holding `values`; the decoded rows come back as plain objects keyed `v`. */
const column = (values) => ({ typeName: 't', fields: ['v'], rows: values.map((v) => [v]) })
const rowsOf = (values) => ({ t: values.map((v) => ({ v })) })

void describe('michi output decodes to the same values in the reference decoder', () => {
  void it('plain tables, several columns and rows', () => {
    const got = roundTrip({
      typeName: 'issue',
      fields: ['number', 'title', 'open', 'score'],
      rows: [
        [i(1), s('Fix login'), b(true), f(1.5)],
        [i(-2), s('Add search'), b(false), f(0.25)],
      ],
    })
    assert.deepEqual(got, {
      issue: [
        { number: 1, title: 'Fix login', open: true, score: 1.5 },
        { number: -2, title: 'Add search', open: false, score: 0.25 },
      ],
    })
  })

  void it('a table with no rows', () => {
    assert.deepEqual(roundTrip({ typeName: 't', fields: ['a', 'b'], rows: [] }), { t: [] })
  })

  void it('delimiters, quotes and backslashes inside strings', () => {
    const values = ['a,b', 'say "hi"', 'back\\slash', 'trail\\', '"', ',', '\\"', 'a:b', 'key: value', '[1]', '{x}', '- item', '# not a comment']
    assert.deepEqual(roundTrip(column(values.map(s))), rowsOf(values))
  })

  void it('newlines, carriage returns, tabs and other whitespace', () => {
    const values = ['l1\nl2', 'a\r\nb', 'tab\there', '\n', '\t', ' lead', 'trail ', '  both  ', ' nbsp', ' sep', ' para']
    assert.deepEqual(roundTrip(column(values.map(s))), rowsOf(values))
  })

  void it('Unicode, including astral characters and combining marks', () => {
    const values = ['café', 'naïve', '日本語', '🙂', 'a🙂b', 'é', '​zwsp', 'עברית', '🏳️‍🌈']
    assert.deepEqual(roundTrip(column(values.map(s))), rowsOf(values))
  })

  void it('strings that look like other types stay strings', () => {
    const values = ['42', '-7', '3.14', '1e5', '0x1F', '007', 'true', 'false', 'null', 'NaN', 'Infinity', '-', '', ' ', '-0', '.5', '5.']
    assert.deepEqual(roundTrip(column(values.map(s))), rowsOf(values))
  })

  void it('real null, booleans and numbers keep their type', () => {
    const got = roundTrip({
      typeName: 't',
      fields: ['n', 'bt', 'bf', 'i', 'neg', 'zero', 'fl'],
      rows: [[nul, b(true), b(false), i(42), i(-42), i(0), f(2.5)]],
    })
    assert.deepEqual(got, { t: [{ n: null, bt: true, bf: false, i: 42, neg: -42, zero: 0, fl: 2.5 }] })
  })

  void it('null is distinct from the empty string and from the string "null"', () => {
    const got = roundTrip(column([nul, s(''), s('null')]))
    assert.deepEqual(got, rowsOf([null, '', 'null']))
  })

  void it('floats at the edges of the range', () => {
    const values = [0.1, 1e21, 1e-7, 5e-324, 1.7976931348623157e308, -1.5, 123456789.125]
    assert.deepEqual(roundTrip(column(values.map(f))), rowsOf(values))
  })

  void it('integers across the safe range', () => {
    const values = [0, 1, -1, 255, 2 ** 31, -(2 ** 31), Number.MAX_SAFE_INTEGER, Number.MIN_SAFE_INTEGER]
    assert.deepEqual(roundTrip(column(values.map(i))), rowsOf(values))
  })

  void it('field names that need quoting', () => {
    const fields = ['plain', 'with space', 'with,comma', 'a:b', 'quote"d', '1st', 'dash-ed', 'dot.ted', 'ünï', '']
    const rows = [fields.map((_, n) => i(n))]
    const got = roundTrip({ typeName: 't', fields, rows })
    assert.deepEqual(got, { t: [Object.fromEntries(fields.map((k, n) => [k, n]))] })
  })

  void it('the totalCount line and the help block alongside the table', () => {
    const got = roundTrip({
      typeName: 'item',
      fields: ['id'],
      rows: [[i(1)], [i(2)]],
      totalCount: 40,
      hints: ['next: page=2', 'comma, inside', 'say "x"', 'colon: here', ''],
    })
    assert.deepEqual(got, {
      item: [{ id: 1 }, { id: 2 }],
      totalCount: 40,
      help: ['next: page=2', 'comma, inside', 'say "x"', 'colon: here', ''],
    })
  })

  void it('a help block on its own', () => {
    const hints = ['a', 'b,c', 'd "e"', 'f: g']
    assert.deepEqual(decode(renderHints(hints)), { help: hints })
  })

  void it('a truncated cell decodes to the shortened text plus michi\'s marker', () => {
    const long = 'x'.repeat(500)
    const got = roundTrip({ ...column([s(long)]), truncationHint: 'full=true' })
    const cell = got.t[0].v
    assert.equal(typeof cell, 'string')
    assert.ok(cell.startsWith('x'.repeat(100)), 'keeps the head of the text')
    assert.ok(cell.length < long.length, 'is shorter than the input')
    assert.ok(cell.includes('use full=true'), `carries the truncation hint: ${cell.slice(-60)}`)
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
  'true', 'false', 'null', 'é', '日', '🙂', ' ', ' ', '​', '\u0001', '\u007f',
]

const randomString = (rand) => {
  let out = ''
  for (let n = Math.floor(rand() * 10); n > 0; n--) out += PIECES[Math.floor(rand() * PIECES.length)]
  return out
}

void describe('fuzzed strings', () => {
  // Property: for any input, michi either rejects it up front (a validation error) or emits text the reference
  // decoder reads back as exactly the input. Never silently different, never undecodable.
  void it('round-trip in cells, field names and hints, or are rejected before rendering', () => {
    const seed = 20261024
    const rand = prng(seed)
    let accepted = 0
    let rejected = 0
    for (let n = 0; n < 2000; n++) {
      const cells = [randomString(rand), randomString(rand), randomString(rand)]
      const fields = ['a', 'b', 'c']
      const hints = [randomString(rand)]
      const label = `seed ${seed}, case ${n}: ${JSON.stringify({ cells, hints })}`
      let text
      try {
        text = renderToon({ typeName: 't', fields, rows: [cells.map(s)], hints })
      } catch (err) {
        assert.match(String(err.message), /toon_validation_failed/, `unexpected error type, ${label}`)
        rejected++
        continue
      }
      accepted++
      const got = decode(text)
      assert.deepEqual(got.t, [{ a: cells[0], b: cells[1], c: cells[2] }], label)
      assert.deepEqual(got.help, hints, label)
    }
    assert.ok(accepted > 1500, `most generated inputs should render (accepted ${accepted}, rejected ${rejected})`)
  })

  void it('field names round-trip or are rejected before rendering', () => {
    const rand = prng(777)
    let accepted = 0
    for (let n = 0; n < 1000; n++) {
      const name = randomString(rand)
      const label = `field ${JSON.stringify(name)}`
      let text
      try {
        text = renderToon({ typeName: 't', fields: [name], rows: [[i(1)]], hints: [] })
      } catch (err) {
        assert.match(String(err.message), /toon_validation_failed/, label)
        continue
      }
      accepted++
      assert.deepEqual(decode(text), { t: [{ [name]: 1 }] }, label)
    }
    assert.ok(accepted > 0)
  })
})
