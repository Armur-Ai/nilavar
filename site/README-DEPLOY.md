# Nilavar website — deploy

Static site, no build step, no third-party requests.

## Cloudflare Pages (drag & drop)
1. Cloudflare dashboard → **Workers & Pages** → **Create** → **Pages** →
   **Upload assets**.
2. Name the project (e.g. `nilavar`) and drop this folder (or the zip's
   contents, with `index.html` at the top level).
3. Deploy. `_headers` sets a strict Content-Security-Policy automatically.

## Wrangler CLI
```bash
npx wrangler pages deploy . --project-name nilavar
```

Download buttons point at
`https://github.com/Armur-Ai/nilavar/releases/latest/download/<asset>`.
