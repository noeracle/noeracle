import { themes as prismThemes } from 'prism-react-renderer';
import type { Config } from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Noeracle',
  tagline: 'The pull-based price oracle for Stellar.',
  favicon: 'img/favicon.svg',

  future: {
    v4: true,
    faster: true,
  },

  url: 'https://docs.noeracle.org',
  baseUrl: '/',

  organizationName: 'noeracle',
  projectName: 'noeracle',

  onBrokenLinks: 'warn',

  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  // Load Geist + Instrument Serif + JetBrains Mono from Google Fonts.
  // CDN <link> tags keep the Docusaurus build fast — no @import at build time.
  headTags: [
    {
      tagName: 'link',
      attributes: { rel: 'preconnect', href: 'https://fonts.googleapis.com' },
    },
    {
      tagName: 'link',
      attributes: {
        rel: 'preconnect',
        href: 'https://fonts.gstatic.com',
        crossorigin: 'anonymous',
      },
    },
    {
      tagName: 'link',
      attributes: {
        rel: 'stylesheet',
        href: 'https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap',
      },
    },
  ],

  presets: [
    [
      'classic',
      {
        docs: {
          path: '../docs',
          routeBasePath: '/',
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/noeracle/noeracle/edit/main/docs/',
          showLastUpdateTime: false,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
        sitemap: {
          changefreq: 'weekly',
          priority: 0.5,
        },
      } satisfies Preset.Options,
    ],
  ],

  plugins: [
    [
      'docusaurus-plugin-copy-page-button',
      {
        placement: 'article',
        enabledActions: ['copy', 'view', 'chatgpt', 'claude'],
        generateMarkdownRoutes: true,
        labels: {
          button: { label: 'Copy page' },
          copy: {
            title: 'Copy page',
            description: 'Copy page as Markdown for LLMs',
          },
          view: {
            title: 'View as Markdown',
            description: 'View this page as plain text',
          },
          chatgpt: {
            title: 'Open in ChatGPT',
            description: 'Ask ChatGPT about this page',
          },
          claude: {
            title: 'Open in Claude',
            description: 'Ask Claude about this page',
          },
        },
      },
    ],
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        indexBlog: false,
        docsDir: '../docs',
        docsRouteBasePath: '/',
        highlightSearchTermsOnTargetPage: true,
        explicitSearchResultPath: true,
        searchResultLimits: 10,
      },
    ],
  ],

  themeConfig: {
    image: 'img/og-image.png',
    metadata: [
      { name: 'theme-color', content: '#050505' },
      { name: 'twitter:card', content: 'summary_large_image' },
      {
        name: 'description',
        content:
          'Pull-based price oracle for Stellar. Sub-500 ms freshness, verified inside your transaction.',
      },
      {
        property: 'og:description',
        content:
          'Pull-based price oracle for Stellar. Sub-500 ms freshness, verified inside your transaction.',
      },
    ],
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Noeracle',
      logo: {
        alt: 'Noeracle',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/reference/sdk',
          label: 'SDK',
          position: 'left',
        },
        {
          to: '/reference/contract',
          label: 'Contract',
          position: 'left',
        },
        {
          href: 'https://noeracle.org',
          label: 'Home',
          position: 'right',
        },
        {
          href: 'https://www.npmjs.com/package/@noeracle/sdk',
          label: 'npm',
          position: 'right',
        },
        {
          href: 'https://github.com/noeracle/noeracle',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            { label: 'Quickstart', to: '/get-started/quickstart' },
            { label: 'Integration patterns', to: '/integration/patterns' },
            { label: 'SDK reference', to: '/reference/sdk' },
            { label: 'Contract reference', to: '/reference/contract' },
          ],
        },
        {
          title: 'Resources',
          items: [
            { label: 'GitHub', href: 'https://github.com/noeracle/noeracle' },
            { label: 'npm', href: 'https://www.npmjs.com/package/@noeracle/sdk' },
            {
              label: 'Attestation service',
              href: 'https://api.noeracle.org/health',
            },
            {
              label: 'Testnet contract',
              href: 'https://stellar.expert/explorer/testnet/contract/CAYIP67UDVX5UPXGN3XDAWVIEFBAVG6G7LUESEOU3NUQKTWN55W34YBG',
            },
          ],
        },
        {
          title: 'Project',
          items: [
            { label: 'Architecture', to: '/concepts/architecture' },
            { label: 'Threat model', to: '/concepts/threat-model' },
            { label: 'Roadmap', to: '/project/roadmap' },
            { label: 'Reflector', href: 'https://reflector.network' },
          ],
        },
      ],
      copyright: `Noeracle — MIT-licensed. Built on Stellar.`,
    },
    prism: {
      theme: prismThemes.vsDark,
      darkTheme: prismThemes.vsDark,
      additionalLanguages: ['rust', 'bash', 'toml', 'json'],
    },
    tableOfContents: {
      minHeadingLevel: 2,
      maxHeadingLevel: 4,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
