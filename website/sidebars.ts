import type { SidebarsConfig } from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docs: [
    'intro',
    {
      type: 'category',
      label: 'Get started',
      collapsed: false,
      items: ['get-started/quickstart'],
    },
    {
      type: 'category',
      label: 'Integration',
      collapsed: false,
      items: ['integration/patterns'],
    },
    {
      type: 'category',
      label: 'Concepts',
      collapsed: false,
      items: ['concepts/architecture', 'concepts/threat-model'],
    },
    {
      type: 'category',
      label: 'Reference',
      collapsed: false,
      items: ['reference/sdk', 'reference/contract'],
    },
    {
      type: 'category',
      label: 'Project',
      collapsed: false,
      items: ['project/roadmap'],
    },
  ],
};

export default sidebars;
