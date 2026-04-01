#!/usr/bin/env node
// CDP-based article extraction: connects to user's Chrome (must have --remote-debugging-port=9222)
import puppeteer from 'puppeteer';

const url = process.argv[2];
const port = process.argv.includes('--port')
  ? process.argv[process.argv.indexOf('--port') + 1]
  : '9222';
const timeout = process.argv.includes('--timeout')
  ? parseInt(process.argv[process.argv.indexOf('--timeout') + 1])
  : 15000;

if (!url) {
  process.stderr.write('Usage: node cdp-extract.mjs <url> [--port 9222] [--timeout 15000]\n');
  process.exit(1);
}

async function extract() {
  let browser, page;
  try {
    browser = await puppeteer.connect({
      browserURL: `http://127.0.0.1:${port}`,
    });
  } catch (e) {
    process.stderr.write(
      `Cannot connect to Chrome CDP on port ${port}.\n` +
      `Launch Chrome with: chrome.exe --remote-debugging-port=${port} --user-data-dir="%LOCALAPPDATA%\\Google\\Chrome\\User Data"\n` +
      `Or run: scripts/chrome-cdp-setup.bat\n`
    );
    process.exit(1);
  }

  try {
    page = await browser.newPage();
    await page.goto(url, { waitUntil: 'networkidle2', timeout });

    // Try selectors from most specific to least specific
    const selectors = ['article', 'main', '[role="main"]', '.post-content', '.article-content', '.article-body', '.entry-content', '.story-body', 'body'];
    let bestHtml = '';

    for (const selector of selectors) {
      try {
        const html = await page.$eval(selector, el => el.innerHTML);
        if (html && html.length > bestHtml.length) {
          bestHtml = html;
        }
        // If we got good content from a specific selector, stop
        if (selector !== 'body' && bestHtml.length > 200) {
          break;
        }
      } catch {
        // selector not found, try next
      }
    }

    if (!bestHtml) {
      process.stderr.write('No article content found on page\n');
      process.exit(3);
    }

    process.stdout.write(bestHtml);
  } catch (e) {
    process.stderr.write(`Navigation/extraction failed: ${e.message}\n`);
    process.exit(2);
  } finally {
    if (page) await page.close().catch(() => {});
    if (browser) browser.disconnect();
  }
}

extract();
