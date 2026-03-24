// inject-layout.ts — Reference snippet for adding SSHX observability to a Next.js app.
//
// Copy the relevant parts into your app/layout.tsx.
//
// The overlay loads automatically in dev mode (NODE_ENV=development).
// To disable: NEXT_PUBLIC_SSHX_OVERLAY=0
// To force-enable in production: NEXT_PUBLIC_SSHX_OVERLAY=1
//
// Set NEXT_PUBLIC_SSHX_SERVER to point to your sshx server
// (e.g. "https://sshx.example.com:8051"). The scripts are served from there.
//
// Users paste the SSHX session URL into the overlay panel at runtime.
// No env var needed for the URL itself.

// --- app/layout.tsx -----------------------------------------------------------

import Script from 'next/script';

export default function RootLayout({ children }: { children: React.ReactNode }) {
  // Enabled by default in dev, disabled in production.
  // Set NEXT_PUBLIC_SSHX_OVERLAY=0 to disable, any other value to force-enable.
  const overlayEnv = process.env.NEXT_PUBLIC_SSHX_OVERLAY;
  const isDev = process.env.NODE_ENV === 'development';
  const showOverlay = overlayEnv === '0' ? false : (overlayEnv ? true : isDev);

  // The sshx server origin — scripts are served from there.
  const sshxServer = process.env.NEXT_PUBLIC_SSHX_SERVER || '';

  return (
    <html lang="en">
      <head>
        {showOverlay && (
          <>
            {/* SSHX component inspector — toggle with Alt+I, paste URL in panel */}
            <Script src={`${sshxServer}/sshx-overlay.js`} strategy="afterInteractive" />
            <Script src={`${sshxServer}/sshx-connect.js`} strategy="afterInteractive" />
          </>
        )}
      </head>
      <body>{children}</body>
    </html>
  );
}
