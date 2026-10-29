// Sign a plugin manifest with Ed25519.
//
//   bun sign.mjs <manifest> <signature> <key.pem> <public-key.txt>
//
// The key is generated on first use and left in `dist/` (gitignored). Pass the
// same key file to later packages so upgrades keep the same publisher.
import { createPrivateKey, createPublicKey, generateKeyPairSync, sign } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'

const [manifestPath, signaturePath, keyPath, publicPath] = process.argv.slice(2)
if (!manifestPath || !signaturePath || !keyPath || !publicPath) {
  console.error('usage: sign.mjs <manifest> <signature> <key.pem> <public-key.txt>')
  process.exit(2)
}

let pem
if (existsSync(keyPath)) {
  pem = readFileSync(keyPath, 'utf8')
} else {
  pem = generateKeyPairSync('ed25519').privateKey.export({ format: 'pem', type: 'pkcs8' })
  writeFileSync(keyPath, pem, { mode: 0o600 })
}
const privateKey = createPrivateKey(pem)
const publicKey = createPublicKey(privateKey)
const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)
const publicHex = Buffer.from(raw).toString('hex')
const signature = sign(null, readFileSync(manifestPath), privateKey)
writeFileSync(signaturePath, signature.toString('hex'))
writeFileSync(publicPath, `${publicHex}\n`)
console.log(publicHex)
