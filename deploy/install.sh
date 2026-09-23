#!/usr/bin/env bash
# One-shot installer for self-ai-gui on Ubuntu.
# Usage: sudo ./deploy/install.sh   (build first with `make build`)
#
# The API key is written to /etc/self-ai-gui/env (mode 0640, root:self-ai) and
# never printed. Pass it in the environment, or type it at the prompt.
set -euo pipefail

BIN="self-ai-gui"
BIN_DIR="/opt/self-ai-gui"
DATA_DIR="/var/lib/self-ai-gui"
ENV_DIR="/etc/self-ai-gui"
ENV_FILE="$ENV_DIR/env"
USER="self-ai"
SERVICE="self-ai-gui.service"

ADDR="${SELF_ADDR:-127.0.0.1:8090}"
PORT="${ADDR##*:}"

BASE_URL="${SELF_BASE_URL:-}"
MODEL="${SELF_MODEL:-}"
MODELS="${SELF_MODELS:-}"
PERSON="${SELF_USER_NAME:-}"
API_KEY="${SELF_API_KEY:-}"

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $EUID -ne 0 ]]; then
  echo "error: run as root (sudo ./deploy/install.sh)" >&2
  exit 1
fi

if [[ ! -f "$REPO_DIR/$BIN" ]]; then
  echo "error: '$BIN' not found in $REPO_DIR. Run 'make build' first." >&2
  exit 1
fi

echo "==> Creating service user"
if ! id -u "$USER" >/dev/null 2>&1; then
  useradd --system --home-dir "$BIN_DIR" --shell /usr/sbin/nologin "$USER"
fi

echo "==> Installing binary"
install -d -m 0755 "$BIN_DIR"
install -m 0755 "$REPO_DIR/$BIN" "$BIN_DIR/$BIN"
install -d -m 0750 -o "$USER" -g "$USER" "$DATA_DIR"

echo "==> Asking for the model server details"
if [[ -z "$BASE_URL" ]]; then
  read -r -p "Model server base URL [https://api.openai.com/v1]: " BASE_URL
  BASE_URL="${BASE_URL:-https://api.openai.com/v1}"
fi
if [[ -z "$API_KEY" ]]; then
  if [[ "$BASE_URL" == *"127.0.0.1"* || "$BASE_URL" == *"localhost"* ]]; then
    echo "     a local model server usually needs no key"
  fi
  read -r -s -p "API key (Enter to skip): " API_KEY
  echo
fi
if [[ -z "$MODEL" ]]; then
  read -r -p "Default model [gpt-4o-mini]: " MODEL
  MODEL="${MODEL:-gpt-4o-mini}"
fi

echo "==> Writing $ENV_FILE"
install -d -m 0750 -o root -g "$USER" "$ENV_DIR"
umask 077
{
  echo "# self-ai-gui environment. Contains a secret: keep it 0640 root:$USER."
  echo "SELF_BASE_URL=$BASE_URL"
  echo "SELF_MODEL=$MODEL"
  [[ -n "$MODELS" ]] && echo "SELF_MODELS=$MODELS"
  [[ -n "$PERSON" ]] && echo "SELF_USER_NAME=$PERSON"
  [[ -n "$API_KEY" ]] && echo "SELF_API_KEY=$API_KEY"
} > "$ENV_FILE"
chown root:"$USER" "$ENV_FILE"
chmod 0640 "$ENV_FILE"
unset API_KEY

echo "==> Installing systemd unit"
install -m 0644 "$REPO_DIR/deploy/$SERVICE" "/etc/systemd/system/$SERVICE"
sed -i "s|^Environment=SELF_ADDR=.*|Environment=SELF_ADDR=$ADDR|" "/etc/systemd/system/$SERVICE"
systemctl daemon-reload

echo "==> Starting service"
systemctl enable --now "$SERVICE"
sleep 2

if curl -fsS "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then
  echo "OK: the chat is up on $ADDR"
else
  echo "Started, but the health check failed. Check:"
  echo "  systemctl status $SERVICE --no-pager"
  echo "  journalctl -u $SERVICE -e --no-pager"
fi

echo
echo "Next: put it behind your domain (ai.andrinoff.com), reusing the Caddy that"
echo "already serves your other sites:"
echo "  sudo ./deploy/setup-domain.sh ai.andrinoff.com"
