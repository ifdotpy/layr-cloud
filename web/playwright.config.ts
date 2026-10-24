import { defineConfig } from '@playwright/test';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const outputOption = process.argv.find(arg => arg.startsWith('--output='))?.slice('--output='.length);
const outputIndex = process.argv.indexOf('--output');
const output = outputOption ?? (outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined);
const reportDirectory = output ? resolve(output) : fileURLToPath(new URL('./test-results/', import.meta.url));

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.ts',
  outputDir: './test-results',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: 2,
  reporter: [['list'], ['json', { outputFile: resolve(reportDirectory, 'results.json') }], ['./tests/screenshot-reporter.ts']],
  use: {
    baseURL: 'http://127.0.0.1:4173',
    browserName: 'chromium',
    viewport: { width: 1280, height: 900 },
    colorScheme: 'light',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: 'npm run dev -- --host 127.0.0.1 --port 4173 --strictPort',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
