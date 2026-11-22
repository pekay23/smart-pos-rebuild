// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState } from "react";
import { posApi } from "../lib/api";
import type { ImageEntityType } from "../lib/models";

type ImageChangedDetail = { entityType: ImageEntityType; entityId: string };

export function EntityImageThumbnail({ entityType, entityId, fallback, className }: { entityType: ImageEntityType; entityId: string; fallback: string; className: string }) {
  const elementRef = useRef<HTMLSpanElement>(null);
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let requested = false;
    let visible = false;
    async function load() {
      if (requested) return;
      requested = true;
      try { const image = await posApi.getEntityImageThumbnail(entityType, entityId); if (active) setSrc(image); }
      catch { if (active) setSrc(null); }
    }
    const element = elementRef.current;
    let observer: IntersectionObserver | undefined;
    if (element && "IntersectionObserver" in window) {
      observer = new IntersectionObserver((entries) => {
        if (entries.some((entry) => entry.isIntersecting)) { visible = true; void load(); observer?.disconnect(); }
      }, { rootMargin: "80px" });
      observer.observe(element);
    } else { visible = true; void load(); }
    function onImageChanged(event: Event) {
      const detail = (event as CustomEvent<ImageChangedDetail>).detail;
      if (detail?.entityType !== entityType || detail.entityId !== entityId) return;
      requested = false;
      setSrc(null);
      if (visible) void load();
    }
    window.addEventListener("nimbletill:entity-image-changed", onImageChanged);
    return () => { active = false; observer?.disconnect(); window.removeEventListener("nimbletill:entity-image-changed", onImageChanged); };
  }, [entityId, entityType]);

  return <span className={`entity-image-thumbnail ${className}`} ref={elementRef} aria-hidden="true">{src ? <img src={src} loading="lazy" alt=""/> : fallback}</span>;
}
