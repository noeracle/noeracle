# Noeracle docs site

The `docs.noeracle.org` Docusaurus site. Content is sourced from the top-level
`docs/` directory at the repo root; this directory is the build harness only.

## Stack

- Docusaurus 3.10 (classic preset, TypeScript config)
- `future.faster: true` — Rspack + SWC build pipeline
- `@easyops-cn/docusaurus-search-local` for in-page search (no Algolia key
  required; swap to DocSearch when the application is approved)
- React 19, MDX-first

## Local development

```bash
cd website
npm install      # one-time
npm start        # http://localhost:3000  — hot reload
npm run build    # production build → website/build/
npm run serve    # serve the production build locally
```

The dev server reads from `../docs/**` and reloads on change.

## Where content lives

| Path | Purpose |
|---|---|
| `../docs/intro.mdx` | Site landing (`/`) |
| `../docs/get-started/quickstart.mdx` | Quickstart |
| `../docs/integration/patterns.mdx` | Integration patterns |
| `../docs/concepts/architecture.mdx` | System design |
| `../docs/concepts/threat-model.mdx` | v0 trust model |
| `../docs/reference/sdk.mdx` | TypeScript SDK reference |
| `../docs/reference/contract.mdx` | Soroban contract reference |
| `../docs/project/roadmap.mdx` | v0 → v3 roadmap |
| `sidebars.ts` | Manual sidebar order |
| `docusaurus.config.ts` | Site metadata, navbar, footer, plugins |
| `src/css/custom.css` | Noeracle theme tokens |
| `src/components/NoeracleQuickstart/` | Live `BTC/USD` price ticker (intro + Quickstart) |
| `static/img/` | favicon.svg, logo.svg, og-image.png + og-image.svg |
