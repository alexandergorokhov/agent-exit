#!/usr/bin/env bash
# `make screen`: the README's main offline claims (7 rows), run the way a
# screener would, each with the outcome the README promises. Prints one PASS/FAIL row
# per claim and writes out/screen.json. No network, no keys, no account.
set -u
cd "$(dirname "$0")/.."
mkdir -p out/screen
rows=()
fail=0
run() { # id, README claim, expected exit, grep pattern, command...
  local id=$1 claim=$2 want=$3 pat=$4; shift 4
  local log="out/screen/$id.log" t0=$SECONDS
  "$@" >"$log" 2>&1
  local code=$? ok=1
  [ "$code" = "$want" ] || ok=0
  grep -Eq -- "$pat" "$log" || ok=0
  local res=PASS; [ $ok = 1 ] || { res=FAIL; fail=1; }
  printf '%s %-3s %-62s (exit %s, %ss)\n' "$res" "$id" "$claim" "$code" $((SECONDS - t0)) >&2
  rows+=("{\"id\":\"$id\",\"claim\":\"$claim\",\"command\":\"$*\",\"expected_exit\":$want,\"expected_output\":\"${pat//\\/\\\\}\",\"exit\":$code,\"result\":\"$res\",\"log\":\"$log\"}")
}
run R1 "MCP for a judge's agent: code hash K0 + check_claims K1-K8 pass" 0 "9/9 claims pass" make --no-print-directory judge
run R2 "Playground: the contract core and scan compiled to wasm" 0 "test result: ok" cargo test --quiet --locked --offline -p playground
run R3 "The scan blocks the cheat (hidden full-access key)" 2 "full_access_key_present" make --no-print-directory scan FILE=fixtures/scan/cheat.json
run R4 "30-day business loop, 0 human calls, equals golden" 0 "output equals tests/golden/business-loop.json" make --no-print-directory business-loop
run R5 "MCP tests pass, agent-buys-agent equals golden" 0 "output equals tests/golden/agent-buys-agent.json" make --no-print-directory mcp-test
run R6 "Contract unit tests (fee, lockout, shares, C27-C32)" 0 "test result: ok" cargo test --quiet --locked --offline -p agent-exit
run R7 ".mcp.json matches the deployed testnet world" 0 "matches" node scripts/judge-mcp-config.mjs --check
(IFS=,; printf '{"schema":"screen.v1","money":"xUSD-FAKE","pass":%s,"rows":[%s]}\n' "$([ $fail = 0 ] && echo true || echo false)" "${rows[*]}") > out/screen.json
[ $fail = 0 ] && echo "screen: all 7 rows pass (out/screen.json)" >&2 || echo "screen: FAILED rows above; logs in out/screen/" >&2
exit $fail
