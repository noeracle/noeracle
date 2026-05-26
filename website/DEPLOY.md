# Deploying `docs.noeracle.org`

Everything below is **your** to-do — the docs site is built and verified
locally; what's left is Cloudflare DNS, Vercel project setup, and the
Algolia DocSearch application.

## 1. Vercel project

1. Push the `website/` directory (and the top-level `docs/`) to GitHub on
   `noeracle/noeracle`. The Docusaurus site builds from `website/`; content
   is read from `../docs/`.
2. Go to <https://vercel.com/new> and import `noeracle/noeracle`.
3. Configure:
   - **Framework Preset:** Docusaurus
   - **Root Directory:** `website`
   - **Build Command:** `npm run build` (default)
   - **Output Directory:** `build` (default)
   - **Install Command:** `npm install` (default)
   - **Node.js Version:** 22.x
4. Add environment variables: **none required**.
5. Deploy. Vercel will assign a `*.vercel.app` preview URL — sanity-check it
   loads before wiring DNS.

## 2. Cloudflare DNS — `docs.noeracle.org`

1. In Cloudflare → DNS for `noeracle.org`, add a record:
   - **Type:** CNAME
   - **Name:** `docs`
   - **Target:** `cname.vercel-dns.com`
   - **Proxy status:** **DNS only (grey cloud)** — per the project rule for
     anything pointing at Vercel / Fly.
   - **TTL:** Auto
2. In the Vercel project settings → Domains, add `docs.noeracle.org`. Vercel
   will verify via the CNAME and provision a Let's Encrypt cert
   automatically (~1 min).

Once DNS propagates, `https://docs.noeracle.org` is live with auto-renewed
TLS.

## 3. Algolia DocSearch (optional, recommended)

DocSearch is free for open-source documentation but approval takes 2–7 days
— apply now so it's live before any meaningful traffic.

1. Apply at <https://docsearch.algolia.com/apply>:
   - URL: `https://docs.noeracle.org`
   - Repository: `https://github.com/noeracle/noeracle`
   - License: MIT (open source)
2. When the approval email arrives, Algolia provides three values:
   `appId`, `apiKey` (public search key), `indexName`.
3. Swap the search plugin in `docusaurus.config.ts`:

   Replace the `plugins:` block:

   ```ts
   plugins: [],   // remove the local-search plugin
   ```

   Add to `themeConfig`:

   ```ts
   algolia: {
     appId: 'XXXXXXXXXX',
     apiKey: '0123456789abcdef',
     indexName: 'noeracle',
     contextualSearch: true,
   },
   ```

   Uninstall the local plugin:

   ```bash
   npm uninstall @easyops-cn/docusaurus-search-local
   ```

   Redeploy.

Until DocSearch is approved, the local search plugin (already wired) covers
the site without external dependencies.

## 4. Post-deploy checks

- `https://docs.noeracle.org/` loads, dark theme, OG image renders on share
- `https://docs.noeracle.org/get-started/quickstart` — the live
  `<NoeracleQuickstart />` ticker fetches a price from `api.noeracle.org`
- Search returns hits for `Ed25519`, `freshness`, `update_batch_ed25519_args`
- Internal links (e.g., `/concepts/architecture` ↔ `/concepts/threat-model`)
  resolve
- "Edit this page" links land on `noeracle/noeracle/edit/main/docs/…`

## 5. After hackathon — turn-on items

- **DocSearch** (above) — swap from local search when approved.
- **TypeDoc autogen for SDK reference** — when SDK API surface widens. Add
  to `package.json`:

  ```bash
  npm install -D docusaurus-plugin-typedoc typedoc typedoc-plugin-markdown
  ```

  Plus a plugin entry in `docusaurus.config.ts`:

  ```ts
  ['docusaurus-plugin-typedoc', {
    entryPoints: ['../sdk/src/index.ts'],
    tsconfig: '../sdk/tsconfig.json',
    out: 'reference/sdk-api',
    sidebar: { categoryLabel: 'SDK API' },
  }]
  ```

  The hand-curated `reference/sdk.mdx` stays as the prose reference; the
  TypeDoc tree gives readers the full type surface.

- **Versioning** — when SDK 1.0 or `oracle_v1` ships:

  ```bash
  npm run docusaurus docs:version 0.1
  ```

  Versioning is intentionally off for v0 to avoid PR ceremony while the
  surface still churns.

- **Site analytics** — optional. Vercel Analytics drops in via the
  `@vercel/analytics` package; or use Plausible / Fathom via a custom
  `<head>` script.

## 6. Drift hygiene

The retired Astro Starlight site lives at `site/docs/`. Keep it around
until `docs.noeracle.org` has run for a week without issues, then delete
`site/docs/` and remove the `docs/` rule from `site/scripts/build.sh` so
Vercel stops trying to build it. After that, the only source of truth for
docs content is the top-level `docs/` directory in this repo.
