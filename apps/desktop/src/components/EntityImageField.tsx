// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useState, type ChangeEvent } from "react";
import { ImagePlus, Trash2 } from "lucide-react";
import type { EntityImageController } from "./useEntityImage";
import "./entity-image.css";

const maxFileSize = 8 * 1024 * 1024;

export function EntityImageField({ image, title, shape = "landscape" }: { image: EntityImageController; title: string; shape?: "landscape" | "portrait" }) {
  const [error, setError] = useState("");

  function onFileChange(event: ChangeEvent<HTMLInputElement>) {
    const next = event.target.files?.[0] ?? null;
    event.target.value = "";
    setError("");
    if (!next) return;
    if (!["image/jpeg", "image/png", "image/webp"].includes(next.type)) { setError("Choose a JPEG, PNG, or WebP image."); return; }
    if (next.size > maxFileSize) { setError("Choose an image smaller than 8 MB."); return; }
    image.choose(next);
  }

  const src = image.previewUrl ?? image.imageUrl;
  return <div className={`entity-image-field entity-image-${shape}`}>
    <div className="entity-image-preview">{src ? <img src={src} alt={`${title} preview`}/> : <span aria-hidden="true"><ImagePlus size={22}/></span>}</div>
    <div className="entity-image-controls"><strong>{title}</strong><small>{title === "Customer photo" ? "Optional · only upload with the customer's permission" : "Optional · JPEG, PNG or WebP · up to 8 MB"}</small><div><label className="button button-secondary button-small entity-image-choose">{src ? "Replace image" : "Upload image"}<input type="file" accept="image/jpeg,image/png,image/webp" onChange={onFileChange} aria-label={`${src ? "Replace" : "Upload"} ${title.toLowerCase()}`}/></label>{src && <button className="entity-image-remove" type="button" onClick={() => { image.remove(); setError(""); }}><Trash2 size={13}/>Remove</button>}</div>{error && <span className="entity-image-error" role="alert">{error}</span>}</div>
  </div>;
}
