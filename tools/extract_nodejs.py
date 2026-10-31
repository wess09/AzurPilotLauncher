"""Extract a verified official Node.js archive into an empty staging directory."""

import stat
import sys
import tarfile
import zipfile
from pathlib import PurePosixPath


def extract(archive, destination, root):
    seen = set()
    total = 0

    def check(name, size):
        nonlocal total
        path = PurePosixPath(name)
        if (
            not name
            or "\\" in name
            or ":" in name
            or path.is_absolute()
            or ".." in path.parts
            or not path.parts
            or path.parts[0] != root
        ):
            raise ValueError(f"Unsafe Node.js archive path: {name}")
        for part in path.parts:
            stem = part.split(".")[0].upper()
            if part.endswith((".", " ")) or stem in {
                "CON", "PRN", "AUX", "NUL",
                *(f"COM{i}" for i in range(10)),
                *(f"LPT{i}" for i in range(10)),
            }:
                raise ValueError(f"Unsafe Node.js archive path: {name}")
        key = str(path).casefold()
        if key in seen:
            raise ValueError(f"Duplicate Node.js archive path: {name}")
        seen.add(key)
        total += size
        if len(seen) > 100_000 or total > 2 * 1024**3:
            raise ValueError("Node.js archive exceeds extraction limits")

    if str(archive).endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            for member in bundle.infolist():
                check(member.filename, member.file_size)
                if stat.S_ISLNK(member.external_attr >> 16):
                    raise ValueError("Unexpected link in Node.js ZIP")
            bundle.extractall(destination)
    else:
        with tarfile.open(archive, "r:gz") as bundle:
            def safe_member(member, target):
                check(member.name, member.size)
                return tarfile.data_filter(member, target)

            # Preserve npm/npx's internal links, rejecting links outside staging,
            # device files and other unsafe tar entries (Python >= 3.12).
            bundle.extractall(destination, filter=safe_member)


if __name__ == "__main__":
    extract(*sys.argv[1:])
