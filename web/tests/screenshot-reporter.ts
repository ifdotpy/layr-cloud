import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { FullConfig, Reporter, TestCase, TestResult } from '@playwright/test/reporter';
import { pages } from './pages';
import { views } from './views';

export default class ScreenshotReporter implements Reporter {
  private output = '';
  private results = new Map<string, { status: string; screenshots: { mode: string; path: string }[] }>();

  onBegin(config: FullConfig) { this.output = config.projects[0].outputDir; }

  onTestEnd(test: TestCase, result: TestResult) {
    const suffix = ' renders fixture data without browser or accessibility errors';
    if (!test.title.endsWith(suffix)) return;
    this.results.set(test.title.slice(0, -suffix.length), {
      status: result.status,
      screenshots: result.attachments.filter(attachment => views.some(view => view.name === attachment.name))
        .map(attachment => ({ mode: attachment.name, path: attachment.path! })),
    });
  }

  onEnd() {
    mkdirSync(this.output, { recursive: true });
    writeFileSync(join(this.output, 'page-list.json'), JSON.stringify({
      views,
      pages: pages.map(page => ({
        name: page.name,
        route: `/?source=fixture#${page.path}`,
        ...(this.results.get(page.name) ?? { status: 'not_run', screenshots: [] }),
      })),
    }, null, 2) + '\n');
  }
}
