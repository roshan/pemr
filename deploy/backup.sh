#!/usr/bin/env bash
# Nightly backup for personal-emr:
#   - pg_dump --format=custom of the personal_emr database (uses the postgres
#     superuser, whose password lives inside the postgres-main container per
#     the shared-postgres convention on kant)
#   - tar+zstd of ~/personal-emr/files
# Both go to /mnt/r2/backups/personal-emr/, with 14-day retention.

set -euo pipefail

DATE=$(date -u +%Y-%m-%dT%H%M%SZ)
DEST=/mnt/r2/backups/personal-emr
DB_DIR="$DEST/db"
FILES_DIR_BAK="$DEST/files"

mkdir -p "$DB_DIR" "$FILES_DIR_BAK"

# DB dump
podman exec -e PGPASSWORD=postgres postgres-main \
    pg_dump -U postgres -d personal_emr --format=custom \
    > "$DB_DIR/$DATE.dump"

# Files
if [ -d "$HOME/personal-emr/files" ]; then
    tar -C "$HOME/personal-emr" -cf - files | zstd -T0 -19 -o "$FILES_DIR_BAK/$DATE.tar.zst"
fi

# Retention
find "$DB_DIR"        -name '*.dump'    -mtime +14 -delete
find "$FILES_DIR_BAK" -name '*.tar.zst' -mtime +14 -delete

echo "personal-emr backup OK ($DATE)"
