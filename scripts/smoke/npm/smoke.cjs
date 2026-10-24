// Run by scripts/smoke-published.sh inside a fresh project that ran `npm install @orin-dx/michi@<version>`.
// Usage: node smoke.cjs <version> <expected platform package>
'use strict'
const assert = require('node:assert/strict')
const path = require('node:path')

const [version, platformPackage] = process.argv.slice(2)

// Resolve the way a user's install does, from the project's node_modules.
const michiManifestPath = require.resolve('@orin-dx/michi/package.json')
const michiManifest = require(michiManifestPath)
assert.equal(michiManifest.version, version, 'installed @orin-dx/michi version')

const platformManifestPath = require.resolve(`${platformPackage}/package.json`, {
  paths: [path.dirname(michiManifestPath)],
})
const platformManifest = require(platformManifestPath)
assert.equal(platformManifest.version, version, `${platformPackage} version`)
console.log(`resolved platform package: ${platformManifest.name}@${platformManifest.version}`)

// Loading the module loads the native binary: a missing or broken one throws here.
const { renderToon, emptyState, AgentResponse } = require('@orin-dx/michi')

const toon = renderToon({
  typeName: 'issues',
  fields: ['number', 'title', 'state'],
  rows: [
    [
      { type: 'int', intVal: 51815 },
      { type: 'str', strVal: '[Bug]: Telegram plugin' },
      { type: 'str', strVal: 'open' },
    ],
    [
      { type: 'int', intVal: 51812 },
      { type: 'str', strVal: 'dark mode request' },
      { type: 'str', strVal: 'open' },
    ],
  ],
  totalCount: 8771,
  hints: ['Run `gh-axi issue view <number>` to view an issue'],
})
assert.equal(
  toon,
  [
    'issues[2]{number,title,state}:',
    '  51815,"[Bug]: Telegram plugin",open',
    '  51812,dark mode request,open',
    'totalCount: 8771',
    'help[1]: Run `gh-axi issue view <number>` to view an issue',
  ].join('\n'),
)

assert.equal(emptyState('issues'), 'issues: []\ntotalCount: 0')

const response = new AgentResponse('issues')
response.items(
  [
    [
      { type: 'int', intVal: 1 },
      { type: 'str', strVal: 'one' },
    ],
  ],
  ['id', 'name'],
)
response.hint('Run `next`')
assert.equal(response.renderToon(), 'issues[1]{id,name}:\n  1,one\nhelp[1]: Run `next`')
const result = response.toCallToolResult()
assert.equal(result.isError, false)
assert.ok(result.content.length >= 1)

// An error path crosses the native boundary as a catchable JS error.
assert.throws(
  () => renderToon({ typeName: 'help', fields: ['x'], rows: [[{ type: 'str', strVal: 'v' }]], hints: [] }),
  /toon_validation_failed/,
)

console.log('npm smoke OK')
