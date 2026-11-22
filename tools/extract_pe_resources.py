"""Extract PE resources without modifying or executing the input executable."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import pefile


def label(entry) -> str:
    if entry.name is not None:
        return entry.name.string.decode("utf-8", errors="replace")
    return str(entry.id)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    image = pefile.PE(str(args.executable), fast_load=True)
    image.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"]]
    )
    args.output.mkdir(parents=True, exist_ok=True)
    entries = []

    for resource_type in image.DIRECTORY_ENTRY_RESOURCE.entries:
        type_name = label(resource_type)
        for resource in resource_type.directory.entries:
            resource_name = label(resource)
            for language in resource.directory.entries:
                data_entry = language.data.struct
                payload = image.get_data(data_entry.OffsetToData, data_entry.Size)
                filename = f"type_{type_name}_id_{resource_name}_lang_{language.id}.bin"
                (args.output / filename).write_bytes(payload)
                entries.append(
                    {
                        "type": type_name,
                        "id": resource_name,
                        "language": language.id,
                        "file": filename,
                        "size": len(payload),
                        "sha256": hashlib.sha256(payload).hexdigest(),
                    }
                )

    manifest = {
        "source": str(args.executable.resolve()),
        "source_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "machine": image.FILE_HEADER.Machine,
        "resources": entries,
    }
    (args.output / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Extracted {len(entries)} resources to {args.output.resolve()}")


if __name__ == "__main__":
    main()
