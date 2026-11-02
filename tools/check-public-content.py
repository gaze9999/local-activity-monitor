#!/usr/bin/env python3
"""Check Git objects without printing their contents. Requires Python 3.10+."""

import argparse
import html
import json
import re
import subprocess
import sys
from urllib.parse import unquote


PRIVATE_KEY = re.compile(r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----")
WORKSPACE = re.compile(r"[A-Z]:[/\\]+(?:projects|work|works|workspaces?|users)[/\\]", re.I)
UNICODE_BYTE = re.compile(r"\\u00([0-9a-fA-F]{2})")
PRIVATE_NAMES = {"build.pem", "id_rsa", "id_ed25519", "id_ecdsa"}
PRIVATE_SUFFIXES = (".key", ".p12", ".pfx", ".crx")


def git(*args):
    result = subprocess.run(["git", *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise RuntimeError("Git inspection failed; public-content check did not pass")
    return result.stdout


def private_name(name):
    leaf = name.replace("\\", "/").rsplit("/", 1)[-1].lower()
    return leaf in PRIVATE_NAMES or leaf.endswith(PRIVATE_SUFFIXES)


def der_private_key(data):
    """Recognize standalone RSA, EC and PKCS#8 DER structures, including encrypted PKCS#8."""
    def item(offset, end):
        if offset + 2 > end:
            raise ValueError
        tag, length = data[offset:offset + 2]
        offset += 2
        if length & 0x80:
            count = length & 0x7f
            if not 1 <= count <= 4 or offset + count > end:
                raise ValueError
            length = int.from_bytes(data[offset:offset + count], "big")
            offset += count
        stop = offset + length
        if stop > end:
            raise ValueError
        return tag, data[offset:stop], offset, stop

    try:
        tag, _, start, end = item(0, len(data))
        if tag != 0x30 or end != len(data):
            return False
        fields = []
        while start < end:
            tag, value, _, start = item(start, end)
            fields.append((tag, value))
        tags = [tag for tag, _ in fields]
        if tags[:2] == [0x30, 0x04]:
            # PBES1/PBES2 OID family; do not classify an arbitrary SEQUENCE/OCTET STRING as a key.
            return fields[0][1].startswith(b"\x06\x09\x2a\x86\x48\x86\xf7\x0d\x01\x05")
        if not fields or fields[0] not in [(0x02, b"\x00"), (0x02, b"\x01")]:
            return False
        return (len(tags) >= 9 and tags[:9] == [0x02] * 9
                or tags[:3] == [0x02, 0x30, 0x04]
                or tags[:2] == [0x02, 0x04])
    except (ValueError, IndexError):
        return False


def content_problems(data):
    # NUL removal covers UTF-16/32 ASCII markers, even without a BOM and in binary blobs.
    text = data.replace(b"\x00", b"").decode("utf-8", "replace")
    normalized = UNICODE_BYTE.sub(lambda match: chr(int(match[1], 16)), text)
    normalized = html.unescape(unquote(normalized)).replace("\\/", "/")
    problems = []
    if PRIVATE_KEY.search(text) or PRIVATE_KEY.search(normalized) or der_private_key(data):
        problems.append("private key material")
    if WORKSPACE.search(text) or WORKSPACE.search(normalized):
        problems.append("absolute local workspace path")
    return problems


def fixture_blobs():
    name = ".public-content-fixtures.json"
    if name.encode() not in git("ls-files", "-z").split(b"\x00"):
        return set()
    entries = json.loads(git("show", ":" + name))
    if not isinstance(entries, list):
        raise ValueError("Fixture exceptions must be a list")
    allowed = set()
    for entry in entries:
        if (not isinstance(entry, dict) or set(entry) != {"blob", "path", "reason"}
                or not isinstance(entry["blob"], str)
                or not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", entry["blob"])
                or not isinstance(entry["path"], str) or not entry["path"]
                or not isinstance(entry["reason"], str) or not entry["reason"].strip()):
            raise ValueError("Invalid exact fixture exception")
        allowed.add(entry["blob"])
    return allowed


class Checker:
    def __init__(self):
        self.fixtures = fixture_blobs()
        self.checked = set()
        self.problems = set()
        self.hash_bytes = 32 if git("rev-parse", "--show-object-format").strip() == b"sha256" else 20
        self.reader = subprocess.Popen(["git", "cat-file", "--batch"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)

    def close(self):
        self.reader.stdin.close()
        self.reader.stdout.close()
        if self.reader.wait() != 0:
            raise RuntimeError("Git object reader failed")

    def name(self, name, location):
        if private_name(name):
            self.problems.add((location, "private signing file or generated CRX"))

    def object(self, oid):
        if oid in self.checked:
            return
        self.checked.add(oid)
        self.reader.stdin.write(oid.encode("ascii") + b"\n")
        self.reader.stdin.flush()
        header = self.reader.stdout.readline().split()
        if len(header) != 3:
            raise RuntimeError("Git object unavailable; check did not pass")
        kind, size = header[1], int(header[2])
        data = self.reader.stdout.read(size)
        if len(data) != size or self.reader.stdout.read(1) != b"\n":
            raise RuntimeError("Incomplete Git object; check did not pass")
        location = kind.decode("ascii") + " " + oid[:12]
        if kind == b"tree":
            position = 0
            while position < len(data):
                space = data.index(b" ", position)
                zero = data.index(b"\x00", space)
                self.name(data[space + 1:zero].decode("utf-8", "replace"), location)
                position = zero + 1 + self.hash_bytes
        else:
            for problem in content_problems(data):
                if kind == b"blob" and oid in self.fixtures and problem == "absolute local workspace path":
                    continue
                self.problems.add((location, problem))

    def index(self):
        for entry in git("ls-files", "--stage", "-z").split(b"\x00"):
            if not entry:
                continue
            metadata, filename = entry.split(b"\t", 1)
            mode, oid, stage = metadata.split()
            if stage != b"0":
                raise RuntimeError("Resolve index conflicts before publication")
            # Print object IDs rather than private filenames or content.
            self.name(filename.decode("utf-8", "replace"), "index " + oid.decode()[:12])
            if mode != b"160000":
                self.object(oid.decode("ascii"))

    def history(self, tips=None):
        if tips is None:
            tips = git("for-each-ref", "--format=%(objectname)").decode("ascii").splitlines()
            tips.append(git("rev-parse", "--verify", "HEAD").decode("ascii").strip())
        if not tips:
            return
        if git("rev-parse", "--is-shallow-repository").strip() == b"true":
            raise RuntimeError("Full history required; fetch --unshallow before publication")
        for oid in tips:
            self.object(oid)  # Includes annotated tag messages and direct blob/tree refs.
        for line in git("rev-list", "--objects", "--no-object-names", *tips, "--").decode("ascii").splitlines():
            self.object(line)


def outgoing_tips(stream):
    tips = []
    for line in stream:
        fields = line.split()
        if len(fields) != 4 or not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", fields[1]):
            raise ValueError("Invalid pre-push input; check did not pass")
        if set(fields[1]) != {"0"}:
            tips.append(fields[1])
    return tips


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", action="store_true", help="check staged bytes (default)")
    parser.add_argument("--history", action="store_true", help="check every local ref and all reachable objects")
    parser.add_argument("--pre-push", action="store_true", help="check the full ancestry of every outgoing ref from Git stdin")
    args = parser.parse_args()
    checker = None
    try:
        checker = Checker()
        if args.pre_push:
            checker.history(outgoing_tips(sys.stdin))
        else:
            if args.history:
                checker.history()
            if args.index or not args.history:
                checker.index()
        for location, reason in sorted(checker.problems):
            print(f"FAIL {location}: {reason}", file=sys.stderr)
        print(f"Public content: {'FAIL' if checker.problems else 'PASS'} ({len(checker.checked)} Git objects)")
        return int(bool(checker.problems))
    except (RuntimeError, ValueError, OSError) as error:
        print(f"Public content: ERROR ({error})", file=sys.stderr)
        return 2
    finally:
        if checker:
            checker.close()


if __name__ == "__main__":
    sys.exit(main())
