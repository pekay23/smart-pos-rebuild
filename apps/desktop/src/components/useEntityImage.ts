// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState } from "react";
import { posApi } from "../lib/api";
import type { ImageEntityType } from "../lib/models";

export function useEntityImage(entityType: ImageEntityType) {
  const [entityId, setEntityId] = useState<string | null>(null);
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [file, setFile] = useState<File | null>(null);
  const [removeExisting, setRemoveExisting] = useState(false);
  const previewRef = useRef<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  function clearPreview() {
    if (previewRef.current) URL.revokeObjectURL(previewRef.current);
    previewRef.current = null;
    setPreviewUrl(null);
  }

  useEffect(() => () => { if (previewRef.current) URL.revokeObjectURL(previewRef.current); }, []);
  useEffect(() => {
    let current = true;
    if (entityId) {
      void posApi.getEntityImage(entityType, entityId).then((image) => { if (current) setImageUrl(image?.dataUrl ?? null); }).catch(() => { if (current) setImageUrl(null); });
    }
    return () => { current = false; };
  }, [entityId, entityType, reloadKey]);

  function reset(id: string | null = null) { clearPreview(); setImageUrl(null); setFile(null); setRemoveExisting(false); setEntityId(id); setReloadKey((value) => value + 1); }
  function choose(next: File | null) {
    clearPreview(); setFile(next); setRemoveExisting(false);
    if (next) { const url = URL.createObjectURL(next); previewRef.current = url; setPreviewUrl(url); }
  }
  function remove() {
    if (file) { clearPreview(); setFile(null); return; }
    if (imageUrl) setRemoveExisting(true);
  }
  async function commit(id: string) {
    if (file) {
      const encoded = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onerror = () => reject(new Error("This image could not be read."));
        reader.onload = () => {
          const value = typeof reader.result === "string" ? reader.result : "";
          const separator = value.indexOf(",");
          if (separator < 0) reject(new Error("This image could not be read."));
          else resolve(value.slice(separator + 1));
        };
        reader.readAsDataURL(file);
      });
      const stored = await posApi.setEntityImage({ entityType, entityId: id, imageBase64: encoded });
      setImageUrl(stored.dataUrl); clearPreview(); setFile(null); setRemoveExisting(false);
      window.dispatchEvent(new CustomEvent("nimbletill:entity-image-changed", { detail: { entityType, entityId: id } }));
    } else if (removeExisting) {
      await posApi.deleteEntityImage(entityType, id);
      setImageUrl(null); setRemoveExisting(false);
      window.dispatchEvent(new CustomEvent("nimbletill:entity-image-changed", { detail: { entityType, entityId: id } }));
    }
  }

  return { imageUrl: removeExisting ? null : imageUrl, previewUrl, file, removeExisting, reset, choose, remove, commit };
}

export type EntityImageController = ReturnType<typeof useEntityImage>;
