"""Verify the rebuilt executable embeds every original dialog template intact."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

import pefile


def resources(path: Path) -> dict[tuple[int, int, int], bytes]:
    pe = pefile.PE(str(path), fast_load=True)
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"]]
    )
    result = {}
    for resource_type in pe.DIRECTORY_ENTRY_RESOURCE.entries:
        if resource_type.id is None:
            continue
        for resource in resource_type.directory.entries:
            if resource.id is None:
                continue
            for language in resource.directory.entries:
                data = language.data.struct
                result[(resource_type.id, resource.id, language.id)] = pe.get_data(
                    data.OffsetToData, data.Size
                )
    return result


def version_strings(path: Path) -> dict[str, str]:
    pe = pefile.PE(str(path), fast_load=True)
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_RESOURCE"]]
    )
    values = {}
    for block in pe.FileInfo or []:
        for item in block:
            for table in getattr(item, "StringTable", []):
                values.update(
                    {
                        key.decode("ascii"): value.decode("cp1252", errors="replace")
                        for key, value in table.entries.items()
                    }
                )
    return values


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("original", type=Path)
    parser.add_argument("rebuilt", type=Path)
    args = parser.parse_args()
    original, rebuilt = resources(args.original), resources(args.rebuilt)

    dialogs = sorted(key for key in original if key[0] == 5)
    mismatches = []
    for resource_type, resource_id, language in dialogs:
        expected = original[(resource_type, resource_id, language)]
        actual = rebuilt.get((10, resource_id, language))
        if actual != expected:
            mismatches.append((resource_id, language))

    other_resources = [(3, 1), (14, 103), (24, 1)]
    for resource_type, resource_id in other_resources:
        matching = [key for key in original if key[:2] == (resource_type, resource_id)]
        for key in matching:
            if rebuilt.get(key) != original[key]:
                mismatches.append((resource_id, key[2]))

    original_version = version_strings(args.original)
    rebuilt_version = version_strings(args.rebuilt)
    required_version_fields = {
        "CompanyName",
        "FileDescription",
        "FileVersion",
        "LegalCopyright",
        "ProductName",
        "ProductVersion",
    }
    if not required_version_fields.issubset(original_version):
        raise SystemExit("Original PE version resource is missing expected fields.")
    if any(original_version.get(key) != rebuilt_version.get(key) for key in required_version_fields):
        raise SystemExit("Version resource strings differ from the original PE.")

    machine = pefile.PE(str(args.rebuilt), fast_load=True).FILE_HEADER.Machine
    print(f"rebuilt_machine=0x{machine:04x}")
    print(f"original_dialogs={len(dialogs)}")
    print(f"exact_dialog_payloads={len(dialogs) - sum(1 for item in mismatches if item[0] in {key[1] for key in dialogs})}")
    if mismatches:
        print(f"mismatches={mismatches}")
        raise SystemExit(1)
    print("Dialog templates, icon image/group, and manifest are byte-identical; version strings match.")
    for _, resource_id, language in dialogs:
        payload = original[(5, resource_id, language)]
        print(f"dialog_id={resource_id} size={len(payload)} sha256={hashlib.sha256(payload).hexdigest()}")


if __name__ == "__main__":
    main()
