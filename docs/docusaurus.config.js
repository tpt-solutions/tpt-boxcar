// @ts-check

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'TPT Cloud-Native',
  tagline: 'Open source tooling from local dev to the edge',
  favicon: 'img/favicon.ico',
  url: 'https://docs.tpt-cloud-native.dev',
  baseUrl: '/',
  organizationName: 'tpt-cloud-native',
  projectName: 'tpt-cloud-native',
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
        title: 'TPT Cloud-Native',
        items: [
          { type: 'docSidebar', sidebarId: 'originSidebar', position: 'left', label: 'Origin' },
          { type: 'docSidebar', sidebarId: 'tetherSidebar', position: 'left', label: 'Tether' },
          { type: 'docSidebar', sidebarId: 'scopeSidebar', position: 'left', label: 'Scope' },
          { type: 'docSidebar', sidebarId: 'chiselSidebar', position: 'left', label: 'Chisel' },
          { type: 'docSidebar', sidebarId: 'frontierSidebar', position: 'left', label: 'Frontier' },
          { href: 'https://github.com/tpt-cloud-native/tpt-cloud-native', label: 'GitHub' },
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
            { label: 'GitHub', href: 'https://github.com/tpt-cloud-native/tpt-cloud-native' },
            { label: 'Discussions', href: 'https://github.com/tpt-cloud-native/tpt-cloud-native/discussions' },
          ]},
          { title: 'More', items: [
            { label: 'Contributing', href: 'https://github.com/tpt-cloud-native/tpt-cloud-native/blob/main/CONTRIBUTING.md' },
            { label: 'Apache 2.0 License', href: 'https://github.com/tpt-cloud-native/tpt-cloud-native/blob/main/LICENSE' },
          ]},
        ],
        copyright: `Copyright ${new Date().getFullYear()} TPT Cloud-Native Contributors. Built with Docusaurus.`,
      },
    }),
};

module.exports = config;
