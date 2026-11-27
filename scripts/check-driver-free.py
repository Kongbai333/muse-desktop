import argparse
import gzip
import hashlib
import io
import tarfile
import zipfile
from pathlib import Path


IMAGES = {
    427552: 'e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce',
    364552: 'daad267411ecdc70a0535e274d2c3e9da3d0084bdac7662cb8424dd4a031b4d9',
    222488: 'f7ba89005544be9d85231a9e0d5f23b2d15b3311667e2dad0debd344918a3f80',
    550928: 'd694fa46ab4cfebcb2632d094c7aa97278eef2f8052438621766d863ae98a931',
}


def scan(name, data, matches):
    view = memoryview(data)
    offset = data.find(b'MZ')
    while offset >= 0:
        if offset + 64 <= len(data):
            pe = int.from_bytes(view[offset + 60:offset + 64], 'little')
            if 64 <= pe <= 4096 and view[offset + pe:offset + pe + 4] == b'PE\0\0':
                for size, digest in IMAGES.items():
                    if offset + size <= len(data) and hashlib.sha256(view[offset:offset + size]).hexdigest() == digest:
                        matches.append((name, offset, size, digest))
        offset = data.find(b'MZ', offset + 2)
    if data.startswith(b'PK\x03\x04'):
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for entry in archive.infolist():
                if not entry.is_dir():
                    scan(name + '!' + entry.filename, archive.read(entry), matches)
    elif data.startswith(b'\x1f\x8b'):
        scan(name + '!gzip', gzip.decompress(data), matches)
    elif data[257:262] == b'ustar':
        with tarfile.open(fileobj=io.BytesIO(data), mode='r:') as archive:
            for entry in archive:
                if entry.isfile():
                    scan(name + '!' + entry.name, archive.extractfile(entry).read(), matches)


def main():
    parser = argparse.ArgumentParser(description='Find exact Wintun 0.14.1 DLLs in trusted release inputs and nested ZIP, gzip or tar members.')
    parser.add_argument('paths', nargs='+', type=Path)
    args = parser.parse_args()
    matches = []
    for path in args.paths:
        scan(str(path), path.read_bytes(), matches)
    for name, offset, size, digest in matches:
        print(f'{name}: offset={offset} size={size} sha256={digest}')
    print(f'{len(matches)} known Wintun images')
    return 1 if matches else 0


if __name__ == '__main__':
    raise SystemExit(main())
