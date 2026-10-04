import {
  existsSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { brotliCompressSync, constants, gzipSync } from 'node:zlib'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'
import type { Plugin } from 'vite'

const rootDir = fileURLToPath(new URL('.', import.meta.url))
const outDir = join(rootDir, 'dist')
const assetsDir = join(outDir, 'assets')

const COMPRESSIBLE = /\.(js|css|html|svg|json|woff2?)$/
const ALREADY_COMPRESSED = /\.(br|gz)$/
const MIN_COMPRESS_BYTES = 512
const BROTLI_QUALITY = 11
const GZIP_LEVEL = 9

/**
 * Emits `.br` and `.gz` siblings for every compressible build artifact so
 * Nginx can serve them directly via `brotli_static on` / `gzip_static on`.
 * The 5 Mbps uplink makes serving precompressed files the difference between
 * a cold and a warm page load.
 */
function precompress(): Plugin {
  return {
    name: 'si-precompress',
    apply: 'build',
    closeBundle() {
      // dist/ holds index.html; dist/assets/ holds content-hashed bundles that
      // Nginx serves with immutable caching, so precompressing both pays off.
      const files = [outDir, assetsDir]
        .filter((root) => existsSync(root))
        .flatMap((root) =>
          readdirSync(root)
            .filter(
              (entry) =>
                COMPRESSIBLE.test(entry) && !ALREADY_COMPRESSED.test(entry),
            )
            .map((entry) => join(root, entry))
            .filter((path) => statSync(path).isFile()),
        )

      let written = 0
      for (const path of files) {
        const buf = readFileSync(path)
        if (buf.byteLength < MIN_COMPRESS_BYTES) continue
        written += 1
        writeFileSync(
          `${path}.br`,
          brotliCompressSync(buf, {
            params: {
              [constants.BROTLI_PARAM_QUALITY]: BROTLI_QUALITY,
              [constants.BROTLI_PARAM_SIZE_HINT]: buf.byteLength,
            },
          }),
        )
        writeFileSync(`${path}.gz`, gzipSync(buf, { level: GZIP_LEVEL }))
      }
      console.info(
        `[si-precompress] wrote .br/.gz for ${written}/${files.length} files`,
      )
    },
  }
}

const apiProxy = {
  '/api': {
    target: process.env.VITE_API_TARGET ?? 'http://127.0.0.1:3000',
    changeOrigin: true,
  },
}

export default defineConfig({
  plugins: [vue(), precompress()],
  build: {
    outDir,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return
          if (id.includes('marked') || id.includes('highlight.js')) {
            return 'markdown'
          }
          return 'vendor'
        },
      },
    },
    chunkSizeWarningLimit: 700,
  },
  server: {
    proxy: apiProxy,
  },
  preview: {
    proxy: apiProxy,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/unit/**/*.spec.ts'],
  },
})