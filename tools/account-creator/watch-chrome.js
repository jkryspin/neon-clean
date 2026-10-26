#!/usr/bin/env node
/**
 * Watch Nexon signup page in Chrome
 */

import puppeteer from 'puppeteer';

async function main() {
  console.log('Launching Chrome...');

  const browser = await puppeteer.launch({
    headless: false,
    args: [
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--disable-dev-shm-usage',
      '--disable-gpu'
    ]
  });

  const page = await browser.newPage();
  await page.setViewport({ width: 1920, height: 1080 });

  console.log('Navigating to signup page...');
  await page.goto('https://www.nexon.com/account/en/create', {
    waitUntil: 'domcontentloaded',
    timeout: 30000
  });

  console.log('\n=== Page loaded ===\n');
  console.log('URL:', await page.url());

  // Log initial state
  await logPageState(page);

  // Set up mutation observer to log DOM changes
  await page.evaluate(() => {
    const observer = new MutationObserver((mutations) => {
      mutations.forEach((mutation) => {
        if (mutation.type === 'attributes') {
          const target = mutation.target;
          if (target.classList && target.classList.contains('dropdown-toggle')) {
            console.log(`[DOM] Dropdown toggle changed: ${target.textContent.trim()}`);
          }
          if (target.classList && target.classList.contains('help-block')) {
            console.log(`[DOM] Help block style changed: ${target.style.display}`);
          }
        }
        if (mutation.type === 'childList' && mutation.addedNodes.length > 0) {
          mutation.addedNodes.forEach((node) => {
            if (node.nodeType === 1) {
              const classes = node.classList;
              if (classes && classes.contains('dropdown-menu')) {
                console.log('[DOM] Dropdown menu added');
              }
            }
          });
        }
      });
    });

    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'style']
    });

    window.signupObserver = observer;
  });

  console.log('\n=== Monitoring page changes ===\n');
  console.log('Please interact with the page. The script will log changes.\n');
  console.log('Press Ctrl+C to stop.\n');

  // Keep running
  await new Promise(() => {});
}

async function logPageState(page) {
  const state = await page.evaluate(() => {
    const toggles = document.querySelectorAll('.dropdown-toggle');
    const toggleValues = Array.from(toggles).map(t => ({
      text: t.textContent.trim(),
      hasError: t.classList.contains('error')
    }));

    const helpBlocks = document.querySelectorAll('.help-block');
    const helpMessages = Array.from(helpBlocks).map(h => ({
      text: h.textContent.trim(),
      visible: h.style.display !== 'none'
    }));

    const url = window.location.href;

    return { toggleValues, helpMessages, url };
  });

  console.log('Current state:');
  console.log('  URL:', state.url);
  console.log('  Dropdown values:', state.toggleValues);
  console.log('  Help messages:', state.helpMessages);
  console.log();
}

main().catch(error => {
  console.error('Error:', error);
  process.exit(1);
});
