// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState } from 'react'
import { ImagePlus, Trash2 } from 'lucide-react'
import { deleteHostedEntityImage, getHostedEntityImage, putHostedEntityImage } from './api'
import type { HostedImageEntityType } from './api'

export function HostedEntityImage({ apiBaseUrl, accessToken, tenantId, branchId, entityType, entityId, label, canEdit = false }: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  entityType: HostedImageEntityType
  entityId: string
  label: string
  canEdit?: boolean
}) {
  const [imageUrl, setImageUrl] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [reload, setReload] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    let current = true
    let ownedUrl: string | null = null
    setError(null)
    setImageUrl(null)
    void getHostedEntityImage({ apiBaseUrl, accessToken, tenantId, branchId, entityType, entityId })
      .then((image) => {
        if (!current || image === null) return
        ownedUrl = URL.createObjectURL(image)
        setImageUrl(ownedUrl)
      })
      .catch((reason: unknown) => {
        if (current) setError(reason instanceof Error ? reason.message : 'The image could not be loaded.')
      })
    return () => {
      current = false
      if (ownedUrl) URL.revokeObjectURL(ownedUrl)
    }
  }, [accessToken, apiBaseUrl, branchId, entityId, entityType, reload, tenantId])

  async function upload(file: File) {
    setBusy(true)
    setError(null)
    try {
      await putHostedEntityImage({ apiBaseUrl, accessToken, tenantId, branchId, entityType, entityId, file })
      setReload((value) => value + 1)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'The image could not be uploaded.')
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    setBusy(true)
    setError(null)
    try {
      await deleteHostedEntityImage({ apiBaseUrl, accessToken, tenantId, branchId, entityType, entityId })
      setReload((value) => value + 1)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'The image could not be removed.')
    } finally {
      setBusy(false)
    }
  }

  return <span className="hosted-entity-image-wrap">
    <span className="hosted-entity-image" aria-label={`${label} image`}>
      {imageUrl ? <img src={imageUrl} alt={`${label}`} loading="lazy"/> : <ImagePlus size={15} aria-hidden="true"/>}
    </span>
    {canEdit && <span className="hosted-entity-image-actions">
      <button className="image-action" type="button" title={`Upload image for ${label}`} aria-label={`Upload image for ${label}`} disabled={busy} onClick={() => inputRef.current?.click()}><ImagePlus size={13}/></button>
      {imageUrl && <button className="image-action image-remove-action" type="button" title={`Remove image for ${label}`} aria-label={`Remove image for ${label}`} disabled={busy} onClick={() => void remove()}><Trash2 size={13}/></button>}
      <input ref={inputRef} className="sr-only" type="file" accept="image/jpeg,image/png,image/webp" aria-label={`Choose image for ${label}`} disabled={busy} onChange={(event) => {
        const file = event.currentTarget.files?.item(0)
        event.currentTarget.value = ''
        if (file) void upload(file)
      }}/>
    </span>}
    {error && <span className="hosted-image-error" role="status" title={error}>Image issue</span>}
  </span>
}
