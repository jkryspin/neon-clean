#!/usr/bin/env node
/**
 * Watch a manual signup using Firefox
 */

import { firefox } from 'playwright';

const CONFIG = {
  nexonSignupUrl: 'https://www.nexon.com/account/en/create',
};

async function main() {
  console.log('Launching Firefox...');

  const browser = await firefox.launch({
    headless: false,
    args: ['--no-sandbox']
  });

  const context = await browser.newContext({
    viewport: { width: 1920, height: 1080 }
  });

  const page = await context.newPage();

  console.log('Navigating to signup page...');
  await page.goto(CONFIG.nexonSignupUrl, { waitUntil: 'domcontentloaded', timeout: 30000 });

  console.log('\n=== Page loaded. Please proceed with signup manually ===\n');
  console.log('This script will log page changes as you interact with it.\n');

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
        }
        if (mutation.type === 'childList' && mutation.addedNodes.length > 0) {
          mutation.addedNodes.forEach((node) => {
            if (node.nodeType === 1 && node.tagName === 'DIV') {
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

  console.log('\nWaiting for 5 minutes... Press Ctrl+C to stop.\n');

  // Wait for 5 minutes
  await new Promise(resolve => setTimeout(resolve, 5 * 60 * 1000));

  console.log('\nTimeout reached. Final state:');
  await logPageState(page);

  await browser.close();
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
