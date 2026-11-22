// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

export interface StationSigningKey {
  privateKey: CryptoKey
  publicKeyHex: string
}

interface StoredStationKey {
  privateKey: CryptoKey
  publicKeyHex: string
}

const DATABASE = 'advanced-pos-station-keys'
const STORE = 'keys'

function encodeHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function openKeyDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1)
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) request.result.createObjectStore(STORE)
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error ?? new Error('Station key storage could not be opened.'))
    request.onblocked = () => reject(new Error('Close other Advanced POS tabs to initialize secure station key storage.'))
  })
}

function readStoredKey(database: IDBDatabase, key: string): Promise<StoredStationKey | undefined> {
  return new Promise((resolve, reject) => {
    const transaction = database.transaction(STORE, 'readonly')
    const request = transaction.objectStore(STORE).get(key) as IDBRequest<StoredStationKey | undefined>
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error ?? new Error('The saved station key could not be read.'))
  })
}

function writeStoredKey(database: IDBDatabase, key: string, value: StoredStationKey): Promise<void> {
  return new Promise((resolve, reject) => {
    const transaction = database.transaction(STORE, 'readwrite')
    transaction.objectStore(STORE).put(value, key)
    transaction.oncomplete = () => resolve()
    transaction.onerror = () => reject(transaction.error ?? new Error('The station key could not be saved.'))
    transaction.onabort = () => reject(transaction.error ?? new Error('Saving the station key was interrupted.'))
  })
}

export function stationCryptoAvailable(): boolean {
  return window.isSecureContext && 'indexedDB' in window && 'crypto' in window && 'subtle' in window.crypto
}

export async function getOrCreateStationKey(stationId: string): Promise<StationSigningKey> {
  if (!stationCryptoAvailable()) {
    throw new Error('Secure station setup requires HTTPS or localhost and a browser with Web Crypto support.')
  }

  const database = await openKeyDatabase()
  try {
    const saved = await readStoredKey(database, stationId)
    if (saved) {
      if (saved.privateKey.algorithm.name !== 'Ed25519' || !saved.privateKey.usages.includes('sign')) {
        throw new Error('The saved station key is not a supported signing key. Contact the business owner to replace this station activation.')
      }
      return { privateKey: saved.privateKey, publicKeyHex: saved.publicKeyHex }
    }

    const pair = await crypto.subtle.generateKey({ name: 'Ed25519' }, false, ['sign', 'verify'])
    const publicKey = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey))
    if (publicKey.byteLength !== 32) throw new Error('The browser generated an invalid station key.')

    const value: StoredStationKey = { privateKey: pair.privateKey, publicKeyHex: encodeHex(publicKey) }
    await writeStoredKey(database, stationId, value)
    return { privateKey: pair.privateKey, publicKeyHex: value.publicKeyHex }
  } finally {
    database.close()
  }
}

export async function signStationMessage(privateKey: CryptoKey, message: string): Promise<string> {
  const signature = await crypto.subtle.sign('Ed25519', privateKey, new TextEncoder().encode(message))
  return encodeHex(new Uint8Array(signature))
}

export async function sha256Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(value))
  return encodeHex(new Uint8Array(digest))
}
