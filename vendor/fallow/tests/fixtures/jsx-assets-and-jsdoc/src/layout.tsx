// A Hono-style layout component that emits HTML via JSX. The asset
// references must be tracked so sibling files in `static/` stay reachable.
export const Layout = () => (
  <html>
    <head>
      <link rel="stylesheet" href="/static/style.css" />
      <link rel="modulepreload" href="/static/vendor.js" />
      <script src="/static/app.js"></script>
    </head>
    <body>
      <h1>Hello</h1>
    </body>
  </html>
);
