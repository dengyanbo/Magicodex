"""Match official Windows checkout line endings for SQLx migration checksums."""

import argparse
from pathlib import Path
import sqlite3
import hashlib

parser = argparse.ArgumentParser()
parser.add_argument("--check-database", action="store_true")
args = parser.parse_args()
root = Path(__file__).resolve().parents[1] / "upstream" / "codex-rust-v0.153.4" / "codex-rs" / "state"
changed = 0
for directory in root.iterdir():
    if directory.is_dir() and (directory.name == "migrations" or directory.name.endswith("_migrations")):
        for path in directory.glob("*.sql"):
            raw = path.read_bytes()
            crlf = raw.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
            if raw != crlf:
                path.write_bytes(crlf)
                changed += 1
print(f"Windows SQL migration line endings normalized: {changed} files; SQL statements unchanged.")
if args.check_database:
    database = Path.home() / ".codex" / "state_5.sqlite"
    if database.exists():
        with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
            checked = 0
            for version, checksum in connection.execute("select version,checksum from _sqlx_migrations"):
                candidates = list((root / "migrations").glob(f"{version:04d}_*.sql"))
                if not candidates:
                    continue
                if hashlib.sha384(candidates[0].read_bytes()).digest() != checksum:
                    raise RuntimeError(f"Installed migration {version} does not match this Windows release")
                checked += 1
        print(f"Existing database migration checksums verified read-only: {checked}; no database writes.")
