#!/usr/bin/env bash
# Uso: make_repo.sh <destino> [archivos=100000] [commits=1000]
# Genera repo sintético para pruebas de rendimiento.
set -euo pipefail
dest=$1; files=${2:-100000}; commits=${3:-1000}
rm -rf "$dest"; mkdir -p "$dest"; cd "$dest"
git init -q
for i in $(seq 0 $((files-1))); do
  d="d$((i % 500))/s$((i % 37))"; mkdir -p "$d"; echo "file $i" > "$d/f$i.txt"
done
git add -A; git -c user.name=t -c user.email=t@t commit -qm "initial"
# commits vacíos rápidos vía fast-import
{
  for i in $(seq 1 "$commits"); do
    printf 'commit refs/heads/main\ncommitter t <t@t> %d +0000\ndata 7\nc%06d\n%s\n' $((1700000000+i)) "$i" "$([ "$i" = 1 ] && echo 'from refs/heads/main^0')"
  done
} | git fast-import --quiet --force
git reset -q --hard main
# 100 cambios sin commit para status
for i in $(seq 0 99); do echo changed >> "d$((i % 500))/s$((i % 37))/f$i.txt"; done
