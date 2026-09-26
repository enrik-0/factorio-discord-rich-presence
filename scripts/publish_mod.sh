#!/usr/bin/env bash
# Publica en el Mod Portal el zip que genera scripts/package_mod.py.
#
# Variables de entorno:
#   FACTORIO_MOD_API_KEY  token con el permiso "ModPortal: Upload Mods" (obligatorio)
#   DRY_RUN=1             valida todo pero no sube nada
#
# Si el mod aún no existe en el portal usa init_publish (primera subida); si ya
# existe usa init_upload. Si esa versión ya está publicada, avisa y termina bien
# para que se pueda relanzar el workflow sin duplicar nada.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
API="https://mods.factorio.com"
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

NAME="$(jq -r '.name' "$ROOT/mod/info.json")"
VERSION="$(jq -r '.version' "$ROOT/mod/info.json")"
ZIP="$ROOT/dist/${NAME}_${VERSION}.zip"

[[ -f "$ZIP" ]] || { echo "::error::No existe $ZIP; ejecuta antes scripts/package_mod.py"; exit 1; }
echo "Publicando ${NAME} ${VERSION} ($(wc -c <"$ZIP") bytes)"

if [[ "${DRY_RUN:-}" != "1" && -z "${FACTORIO_MOD_API_KEY:-}" ]]; then
  echo "::error::Falta el secreto FACTORIO_MOD_API_KEY (Settings > Secrets and variables > Actions)"
  exit 1
fi

# El endpoint público responde 404 mientras el mod no exista.
STATUS="$(curl -sS -o "$TMP" -w '%{http_code}' "$API/api/mods/${NAME}/full")"
case "$STATUS" in
  200)
    if jq -e --arg v "$VERSION" '.releases[]? | select(.version == $v)' "$TMP" >/dev/null; then
      echo "::warning::La versión ${VERSION} ya está en el Mod Portal; no se vuelve a subir."
      exit 0
    fi
    INIT="$API/api/v2/mods/releases/init_upload"
    ;;
  404)
    echo "El mod no existe todavía en el portal: primera publicación."
    INIT="$API/api/v2/mods/init_publish"
    ;;
  *)
    echo "::error::El portal respondió HTTP ${STATUS} al consultar el mod"
    exit 1
    ;;
esac

if [[ "${DRY_RUN:-}" == "1" ]]; then
  echo "DRY_RUN: se usaría ${INIT}; no se sube nada."
  exit 0
fi

RESULT="$(curl -sS -d "mod=${NAME}" -H "Authorization: Bearer ${FACTORIO_MOD_API_KEY}" "$INIT")"
UPLOAD_URL="$(jq -r '.upload_url // empty' <<<"$RESULT")"
if [[ -z "$UPLOAD_URL" ]]; then
  echo "::error::No se obtuvo upload_url: $(jq -r '[.error, .message] | map(select(. != null)) | join(": ")' <<<"$RESULT")"
  exit 1
fi

UPLOAD_RESULT="$(curl -sS -F "file=@${ZIP}" "$UPLOAD_URL")"
if [[ "$(jq -r '.success // false' <<<"$UPLOAD_RESULT")" != "true" ]]; then
  echo "::error::La subida falló: $(jq -r '[.error, .message] | map(select(. != null)) | join(": ")' <<<"$UPLOAD_RESULT")"
  exit 1
fi

echo "Subida de $(basename "$ZIP") completada."
