import { sveltekit } from '@sveltejs/kit/vite'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

const backendPort = process.env.PORT ?? '9000'
const backendTarget = `http://127.0.0.1:${backendPort}`

/** Vite dev paths that must stay on the frontend dev server (not proxied to S3). */
const NOT_S3 =
  '^/(?!ui(?:/|$)|@|node_modules/|src/|\\.svelte-kit/|favicon\\.ico|__vite_ping).*'

export default defineConfig({
  plugins: [sveltekit(), tailwindcss()],
  server: {
    proxy: {
      '/api': backendTarget,
      '/healthz': backendTarget,
      '/readyz': backendTarget,
      '/metrics': backendTarget,
      '/iam': backendTarget,
      // Path-style S3 API (ListBuckets, /{bucket}, /{bucket}/{key}, …).
      // Without this, requests like GET /my-bucket hit SvelteKit and return the
      // "did you mean /ui/…?" base-path page instead of S3 XML errors.
      [NOT_S3]: {
        target: backendTarget,
        changeOrigin: true,
      },
    },
  },
})
