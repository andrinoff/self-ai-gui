#!/usr/bin/env bash
# Serve self-ai-gui on your own domain, with a real, publicly-trusted HTTPS
# cert, over a private network. It ADDS a site block to an existing Caddyfile,
# so it is safe on a box where Caddy already serves other sites
# (home.andrinoff.com, server.andrinoff.com, say).
#
#   sudo ./deploy/setup-domain.sh [ai.andrinoff.com]
#
# How:
#   * An A record points the domain at this server's Tailscale IP, grey-cloud,
#     so the site is reachable only inside your tailnet. The script checks that
#     and warns instead of rewriting DNS.
#   * Caddy gets its certificate over the Cloudflare DNS-01 challenge, reusing
#     the token already present in the Caddyfile when there is one.
set -euo pipefail

DOMAIN="${1:-ai.andrinoff.com}"
ADDR="${SELF_ADDR:-127.0.0.1:8090}"
CONF_DIR="/etc/caddy"
CONF="$CONF_DIR/Caddyfile"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $EUID -ne 0 ]]; then
  echo "error: run as root (sudo $0 [$DOMAIN])" >&2
  exit 1
fi

echo "==> Step 1: Caddy"
if ! command -v caddy >/dev/null 2>&1; then
  echo "     installing Caddy from the official repo"
  apt-get install -y --no-install-recommends curl apt-transport-https >/dev/null
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
  apt-get update
  apt-get install -y caddy
fi
if ! caddy list-modules 2>/dev/null | grep -qx 'dns.providers.cloudflare'; then
  echo "error: this Caddy cannot do Cloudflare DNS-01, and no prebuilt was found." >&2
  echo "  Build one on your Mac (in the home repo):  make caddy-dist" >&2
  echo "  then point this script at it:" >&2
  echo "    sudo CADDY_BINARY=~/caddy-linux-amd64 $0 $DOMAIN" >&2
  exit 1
fi

echo "==> Step 2: DNS"
TS_IP="$(tailscale ip -4 2>/dev/null | head -n1 || true)"
RESOLVED="$(getent hosts "$DOMAIN" 2>/dev/null | awk '{print $1}' | head -n1 || true)"
if [[ -z "$RESOLVED" ]]; then
  echo "     warning: $DOMAIN does not resolve yet." >&2
  echo "     Add: Type A | Name ${DOMAIN%%.*} | Content ${TS_IP:-<tailscale ip>} | Proxy OFF" >&2
elif [[ -n "$TS_IP" && "$RESOLVED" != "$TS_IP" ]]; then
  echo "     warning: $DOMAIN resolves to $RESOLVED, but this machine's Tailscale IP is $TS_IP." >&2
  echo "     The certificate will still issue, but the name may not reach this box." >&2
else
  echo "     $DOMAIN -> $RESOLVED (private, as intended)"
fi

echo "==> Step 3: Reuse the Cloudflare token from the existing Caddyfile"
TOKEN=""
if [[ -f "$CONF" ]]; then
  # Takes the whole value, so {env.CF_DNS_API_TOKEN} or an inline token both work.
  TOKEN="$(grep -oE 'dns cloudflare (\{[^}]*\}|[^ }]+)' "$CONF" | head -n1 | awk '{print $3}' || true)"
  if [[ -n "$TOKEN" ]]; then
    echo "     found one already in use"
  fi
fi
if [[ -z "$TOKEN" ]]; then
  read -r -p "Cloudflare API token (Zone:DNS:Edit) for a real cert [Enter to use a local CA]: " TOKEN || true
fi

echo "==> Step 4: Adding the site block for $DOMAIN"
mkdir -p "$CONF_DIR"
if [[ -f "$CONF" ]]; then
  cp "$CONF" "$CONF.bak"
  # Drop any previous block for this domain: re-running must not stack them.
  awk -v dom="$DOMAIN {" '
    $0 == dom         { skip = 1 }
    skip && $0 == "}" { skip = 0; next }
    !skip             { print }
  ' "$CONF.bak" > "$CONF"
fi

if [[ -n "$TOKEN" ]]; then
  TLS_BLOCK=$'tls {\n\t\tdns cloudflare '"$TOKEN"$'\n\t}'
else
  TLS_BLOCK=$'tls internal'
  echo "     no token: using Caddy's local CA"
fi

cat >> "$CONF" <<EOF

$DOMAIN {
	$TLS_BLOCK

	encode zstd gzip

	reverse_proxy $ADDR
}
EOF
unset TOKEN
chown root:caddy "$CONF"
chmod 0640 "$CONF"

echo "==> Step 5: Validating and reloading"
caddy fmt --overwrite "$CONF" >/dev/null
if ! caddy validate --config "$CONF" >/dev/null 2>&1; then
  echo "error: the Caddyfile is not valid. Previous version kept at $CONF.bak" >&2
  caddy validate --config "$CONF" >&2 || true
  exit 1
fi
systemctl enable caddy >/dev/null 2>&1 || true
systemctl reload caddy 2>/dev/null || systemctl restart caddy

echo
echo "==> Done: https://$DOMAIN"
echo "  The certificate is fetched on the first HTTPS request; then check:"
echo "    journalctl -u caddy -e --no-pager"
echo "    caddy list-certificates | grep $DOMAIN"
echo "    curl -s $ADDR/api/health"
echo "  The site block it added is at the end of $CONF (backup: $CONF.bak)."
