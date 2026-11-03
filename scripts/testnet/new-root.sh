#!/usr/bin/env bash
# Create and fund a fresh testnet root for `make demo-setup`, then point
# ~/.agent-exit.env at it. Run by the owner (it creates accounts).
#   bash scripts/testnet/new-root.sh agentexit2.testnet [rounds]
# From PowerShell:
#   wsl -- bash -ic "cd ~/src/near-transparent-agent-market && git pull && bash scripts/testnet/new-root.sh agentexit2.testnet"
# Each faucet account brings about 10 test NEAR; the root gets its own 10
# plus `rounds` (default 3) throwaway accounts deleted into it, about 40.
# The private key is copied from the near CLI keychain into the env file and
# is never printed.
set -euo pipefail

ROOT="${1:?usage: new-root.sh <name>.testnet [rounds]}"
ROUNDS="${2:-3}"
ENV_FILE="${AGENT_EXIT_ENV:-$HOME/.agent-exit.env}"
CRED="$HOME/.near-credentials/testnet/$ROOT.json"

case "$ROOT" in *.testnet) ;; *) echo "root must end in .testnet" >&2; exit 1 ;; esac
command -v near >/dev/null || { echo "near CLI not found: run 'source ~/.bashrc' or use 'bash -ic'" >&2; exit 1; }

echo "== 1/3 create $ROOT (faucet)"
if [ -f "$CRED" ]; then
  echo "   $CRED exists: skipping create"
else
  near account create-account sponsor-by-faucet-service "$ROOT" autogenerate-new-keypair \
    --signature-scheme ed25519 save-to-legacy-keychain network-config testnet create
fi
[ -f "$CRED" ] || { echo "no key file $CRED: create failed (name taken?). Pick another name." >&2; exit 1; }

echo "== 2/3 fund $ROOT with $ROUNDS throwaway faucet accounts"
for i in $(seq 1 "$ROUNDS"); do
  a="agentexit-fund$i-$RANDOM.testnet"
  echo "   round $i/$ROUNDS: $a"
  if near account create-account sponsor-by-faucet-service "$a" autogenerate-new-keypair \
       --signature-scheme ed25519 save-to-legacy-keychain network-config testnet create; then
    sleep 5
    near account delete-account "$a" beneficiary "$ROOT" network-config testnet sign-with-legacy-keychain send \
      || echo "   delete failed for $a (its NEAR stays there); continuing"
  else
    echo "   faucet refused round $i (rate limit?); continuing"
  fi
  sleep 20
done

echo "== 3/3 point $ENV_FILE at $ROOT (key copied, not printed)"
touch "$ENV_FILE"
cp -p "$ENV_FILE" "$ENV_FILE.bak"
chmod 600 "$ENV_FILE" "$ENV_FILE.bak"
KEY=$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).private_key)' "$CRED")
tmp=$(mktemp)
grep -v -E '^(AGENT_EXIT_ROOT|AGENT_EXIT_ROOT_KEY)=' "$ENV_FILE" > "$tmp" || true
{ cat "$tmp"; echo "AGENT_EXIT_ROOT=$ROOT"; echo "AGENT_EXIT_ROOT_KEY=$KEY"; } > "$ENV_FILE"
rm -f "$tmp"; unset KEY
chmod 600 "$ENV_FILE"
echo "   done; the old file is $ENV_FILE.bak"

echo
near account view-account-summary "$ROOT" network-config testnet now 2>&1 | grep -i -E "native|balance" || true
echo "If the balance is under 38 NEAR, run again with: bash scripts/testnet/new-root.sh $ROOT 1"
echo "Then tell Claude: root ready"
