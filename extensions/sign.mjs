// Sign one extension manifest set with Ed25519.
//
//   bun sign.mjs <manifest-dir> <signature> <key.pem> <public-key.txt>
//
// The set is the root `manifest.toml` plus the optional layer manifests under
// `wasm/`, `web/` and `service/`. File names and lengths are part of the
// message, so a boundary cannot be confused with content.
import { createPrivateKey, createPublicKey, generateKeyPairSync, sign } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const [dir, signaturePath, keyPath, publicPath] = process.argv.slice(2)
if (!dir || !signaturePath || !keyPath || !publicPath) {
  console.error('usage: sign.mjs <manifest-dir> <signature> <key.pem> <public-key.txt>')
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

const chunks = [Buffer.from('anth-extension-manifest-v2\0')]
for (const name of ['manifest.toml', 'wasm/manifest.toml', 'web/manifest.toml', 'service/manifest.toml']) {
  const path = join(dir, name)
  if (!existsSync(path)) continue
  const bytes = readFileSync(path)
  const length = Buffer.alloc(8)
  length.writeBigUInt64LE(BigInt(bytes.length))
  chunks.push(Buffer.from(`${name}\0`), length, bytes)
}
const signature = sign(null, Buffer.concat(chunks), privateKey)
writeFileSync(signaturePath, signature.toString('hex'))
writeFileSync(publicPath, `${publicHex}\n`)
console.log(publicHex)
