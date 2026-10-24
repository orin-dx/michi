// Exits 0 if the npm registry lists a provenance attestation for <package>@<version>, 1 if not, 2 on a lookup error.
//
//   node attestation.cjs @orin-dx/michi 0.7.2
//
// The registry returns `dist.attestations` on the version document when the publish carried provenance.
'use strict'

const [name, version] = process.argv.slice(2)
if (!name || !version) {
  console.error('usage: node attestation.cjs <package> <version>')
  process.exit(2)
}

const url = `https://registry.npmjs.org/${name.replace('/', '%2f')}/${version}`
fetch(url)
  .then(async (res) => {
    if (!res.ok) {
      console.error(`registry answered ${res.status} for ${name}@${version}`)
      process.exit(2)
    }
    const doc = await res.json()
    const attestations = doc?.dist?.attestations
    const predicate = attestations?.provenance?.predicateType
    if (typeof attestations?.url === 'string' && typeof predicate === 'string') {
      console.log(`  ${name}@${version}: provenance ${predicate}`)
      process.exit(0)
    }
    console.error(`  ${name}@${version}: NO provenance attestation`)
    process.exit(1)
  })
  .catch((err) => {
    console.error(`lookup failed for ${name}@${version}: ${err.message}`)
    process.exit(2)
  })
