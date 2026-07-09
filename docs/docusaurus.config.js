// @ts-check

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'TPT Boxcar',
  tagline: 'Open source tooling from local dev to the edge',
  favicon: 'img/favicon.ico',
  url: 'https://docs.tpt-boxcar.dev',
  baseUrl: '/',
  organizationName: 'tpt-boxcar',
  projectName: 'tpt-boxcar',
  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'warn',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          sidebarPath: require.resolve('./sidebars.js'),
        },
        blog: false,
        theme: {
          customCss: require.resolve('./src/css/custom.css'),
        },
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      navbar: {
        title: 'TPT Boxcar',
        items: [
          { type: 'docSidebar', sidebarId: 'useCasesSidebar', position: 'left', label: 'Use Cases' },
          { type: 'docSidebar', sidebarId: 'originSidebar', position: 'left', label: 'Origin' },
          { type: 'docSidebar', sidebarId: 'tetherSidebar', position: 'left', label: 'Tether' },
          { type: 'docSidebar', sidebarId: 'scopeSidebar', position: 'left', label: 'Scope' },
          { type: 'docSidebar', sidebarId: 'chiselSidebar', position: 'left', label: 'Chisel' },
          { type: 'docSidebar', sidebarId: 'frontierSidebar', position: 'left', label: 'Frontier' },
          { href: 'https://github.com/tpt-boxcar/tpt-boxcar', label: 'GitHub' },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          { title: 'Products', items: [
            { label: 'Origin', to: '/origin/' },
            { label: 'Tether', to: '/tether/' },
            { label: 'Scope', to: '/scope/' },
            { label: 'Chisel', to: '/chisel/' },
            { label: 'Frontier', to: '/frontier/' },
          ]},
          { title: 'Community', items: [
            { label: 'GitHub', href: 'https://github.com/tpt-boxcar/tpt-boxcar' },
            { label: 'Discussions', href: 'https://github.com/tpt-boxcar/tpt-boxcar/discussions' },
          ]},
          { title: 'More', items: [
            { label: 'Contributing', href: 'https://github.com/tpt-boxcar/tpt-boxcar/blob/main/CONTRIBUTING.md' },
            { label: 'Apache 2.0 License', href: 'https://github.com/tpt-boxcar/tpt-boxcar/blob/main/LICENSE' },
          ]},
        ],
        copyright: `Copyright ${new Date().getFullYear()} TPT Boxcar Contributors. Built with Docusaurus.`,
      },
    }),
};

module.exports = config;
