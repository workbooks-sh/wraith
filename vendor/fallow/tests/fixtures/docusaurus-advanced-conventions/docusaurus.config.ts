export default async function createConfigAsync() {
  return {
    title: "Docusaurus Coverage Fixture",
    url: "https://example.com",
    baseUrl: "/",
    staticDirectories: ["static-assets", "public"],
    i18n: {
      defaultLocale: "en",
      locales: ["en", "fr"],
      path: "translations"
    },
    scripts: [
      "./src/client/custom-script.js",
      "/scripts/runtime.js",
      "https://cdn.example.com/script.js"
    ],
    stylesheets: [
      { href: "./src/css/extra.css" },
      "/styles/local.css",
      "https://cdn.example.com/style.css"
    ],
    clientModules: [
      "./src/client/global-client.js",
      "./src/css/global-client.css"
    ],
    presets: [
      [
        "classic",
        {
          docs: {
            path: "knowledge",
            sidebarPath: "./knowledge-sidebars.ts"
          },
          blog: {
            path: "updates"
          },
          pages: {
            path: "site-pages"
          },
          theme: {
            customCss: ["./src/css/custom.css"]
          }
        }
      ]
    ],
    plugins: [
      [
        "content-docs",
        {
          id: "community",
          path: "community",
          routeBasePath: "community",
          sidebarPath: "./community-sidebars.ts"
        }
      ]
    ]
  };
}
