# Vendored: @hot-labs/near-connect 0.11.4 (MIT)

The page used to import this from a CDN. It is now served from this repo so
the page's Content-Security-Policy can say `script-src 'self'`, and its
SHA-256 is pinned in `SHA256SUMS` (checked by `make web-test`).

Rebuild and compare:

```
npm pack @hot-labs/near-connect@0.11.4 && tar xzf hot-labs-near-connect-0.11.4.tgz
npm i esbuild@0.25.0
npx esbuild package/build/index.js --bundle --format=esm --minify --platform=browser --outfile=near-connect-0.11.4.esm.js
sha256sum near-connect-0.11.4.esm.js
```

The wallet popup fetches wallet manifests from other hosts. The page's CSP
(`connect-src 'self'`) blocks that on purpose until the testnet login is
wired and those hosts are reviewed and allow-listed one by one.
