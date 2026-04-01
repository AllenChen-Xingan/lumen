// tests/a11y-audit.mjs
// Automated accessibility audit using axe-core + Puppeteer
// Usage: node tests/a11y-audit.mjs [url]
// Default URL: http://localhost:3000

import puppeteer from 'puppeteer';
import { AxePuppeteer } from '@axe-core/puppeteer';

const url = process.argv[2] || 'http://localhost:3000';

async function runAudit() {
  console.log('=== Accessibility Audit ===');
  console.log(`URL: ${url}`);
  console.log('');

  const browser = await puppeteer.launch({ headless: true });
  const page = await browser.newPage();

  try {
    await page.goto(url, { waitUntil: 'networkidle0', timeout: 10000 });
  } catch (e) {
    console.error(`FAIL: Cannot reach ${url}`);
    console.error('Start the dev server first: pnpm dev');
    await browser.close();
    process.exit(1);
  }

  const results = await new AxePuppeteer(page)
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'])
    .analyze();

  const { violations, passes, incomplete } = results;

  console.log(`Passes: ${passes.length}`);
  console.log(`Violations: ${violations.length}`);
  console.log(`Incomplete: ${incomplete.length}`);
  console.log('');

  if (violations.length > 0) {
    console.log('--- VIOLATIONS ---');
    for (const v of violations) {
      console.log(`[${v.impact}] ${v.id}: ${v.description}`);
      console.log(`  Help: ${v.helpUrl}`);
      for (const node of v.nodes) {
        console.log(`  Element: ${node.html.substring(0, 100)}`);
        console.log(`  Fix: ${node.failureSummary}`);
      }
      console.log('');
    }
  }

  if (incomplete.length > 0) {
    console.log('--- NEEDS REVIEW ---');
    for (const item of incomplete) {
      console.log(`  [${item.impact}] ${item.id}: ${item.description}`);
    }
    console.log('');
  }

  await browser.close();

  console.log('=== Summary ===');
  if (violations.length === 0) {
    console.log('STATUS: ALL PASS - No accessibility violations found');
    process.exit(0);
  } else {
    const critical = violations.filter(v => v.impact === 'critical').length;
    const serious = violations.filter(v => v.impact === 'serious').length;
    console.log(`STATUS: FAIL - ${critical} critical, ${serious} serious violations`);
    process.exit(1);
  }
}

runAudit().catch(e => {
  console.error('Audit error:', e.message);
  process.exit(1);
});
